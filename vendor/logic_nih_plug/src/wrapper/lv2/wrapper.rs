//! The main LV2 wrapper implementation.

use std::collections::HashMap;
use std::ffi::c_void;
use std::os::raw::c_char;
use std::ptr;
use std::sync::Arc;

use atomic_refcell::AtomicRefCell;
use parking_lot::Mutex;

use crate::audio_setup::BufferConfig;
use crate::params::internals::ParamPtr;
use crate::plugin::lv2::Lv2Plugin;
use crate::plugin::Plugin;
use crate::prelude::{
    AudioIOLayout, NoteEvent, Params, PluginNoteEvent,
};
use crate::wrapper::state::PluginState;
use crate::wrapper::param_automation::{ParamAutomationHandler, ParamChangeNotifier};

use super::atom::{parse_atom_sequence, write_atom_sequence};
use super::context::{Lv2InitContext, Lv2ProcessContext};
use super::descriptor::{generate_port_descriptors, Lv2PortDescriptor, Lv2PortType};
use super::state::{create_state_interface, Lv2StateHandler, Lv2StateInterface};

/// LV2 plugin handle
pub type Lv2Handle = *mut c_void;

/// LV2 feature structure
#[repr(C)]
pub struct Lv2Feature {
    pub uri: *const c_char,
    pub data: *mut c_void,
}

/// LV2 descriptor structure
#[repr(C)]
pub struct Lv2Descriptor {
    pub uri: *const c_char,
    pub instantiate: extern "C" fn(
        descriptor: *const Lv2Descriptor,
        sample_rate: f64,
        bundle_path: *const c_char,
        features: *const *const Lv2Feature,
    ) -> Lv2Handle,
    pub connect_port: extern "C" fn(instance: Lv2Handle, port: u32, data: *mut c_void),
    pub activate: Option<extern "C" fn(instance: Lv2Handle)>,
    pub run: extern "C" fn(instance: Lv2Handle, sample_count: u32),
    pub deactivate: Option<extern "C" fn(instance: Lv2Handle)>,
    pub cleanup: extern "C" fn(instance: Lv2Handle),
    pub extension_data: Option<extern "C" fn(uri: *const c_char) -> *const c_void>,
}

/// The main LV2 wrapper struct
pub struct Lv2Wrapper<P: Plugin> {
    /// The wrapped plugin instance
    plugin: Arc<Mutex<P>>,
    /// Plugin parameters
    params: Arc<dyn Params>,
    /// Parameter map (ID -> pointer)
    param_map: HashMap<String, ParamPtr>,
    /// Parameter hash map (ID -> hash)
    param_hash_map: HashMap<String, u32>,
    /// Parameter index map (port index -> param ID)
    param_by_port: HashMap<u32, String>,
    /// Port descriptors
    port_descriptors: Vec<Lv2PortDescriptor>,
    /// Audio input buffers (pointers to host-provided buffers)
    audio_inputs: Vec<*mut f32>,
    /// Audio output buffers (pointers to host-provided buffers)
    audio_outputs: Vec<*mut f32>,
    /// Control input ports (pointers to host-provided control values)
    control_inputs: HashMap<u32, *mut f32>,
    /// MIDI input port (pointer to atom sequence)
    midi_input: Option<*mut c_void>,
    /// MIDI output port (pointer to atom sequence)
    midi_output: Option<*mut c_void>,
    /// Current sample rate
    sample_rate: f64,
    /// Current buffer size
    buffer_size: u32,
    /// Whether the plugin is activated
    is_activated: bool,
    /// Audio IO layout
    audio_io_layout: AudioIOLayout,
    /// Input events buffer
    input_events: Vec<PluginNoteEvent<P>>,
    /// Output events buffer
    output_events: Vec<PluginNoteEvent<P>>,
    /// Unified parameter automation handler
    param_automation: ParamAutomationHandler,
}

impl<P: Plugin + Lv2Plugin> Lv2StateHandler for Lv2Wrapper<P> {
    fn get_state(&self) -> PluginState {
        unsafe {
            crate::wrapper::state::serialize_object::<P>(
                self.params.clone(),
                self.param_map.iter().map(|(id, ptr)| (id, *ptr)),
            )
        }
    }

    fn set_state(&mut self, mut state: PluginState) {
        let params = self.params.clone();
        let param_map = &self.param_map;
        unsafe {
            crate::wrapper::state::deserialize_object::<P>(
                &mut state,
                params,
                |id| param_map.get(id).copied(),
                None,
            );
        }
    }
}

impl<P: Plugin + Lv2Plugin> Lv2Wrapper<P> {
    /// Create a new LV2 wrapper instance
    pub fn new(sample_rate: f64) -> Box<Self> {
        let plugin = P::default();
        let params = plugin.params();

        // Build parameter map
        let mut param_map = HashMap::new();
        let mut param_hash_map = HashMap::new();
        let param_ptr_iter = params.param_map();
        for (id, ptr, _group) in param_ptr_iter {
            let id_string = id.to_string();
            let hash = crate::wrapper::util::hash_param_id(&id_string);
            param_map.insert(id_string.clone(), ptr);
            param_hash_map.insert(id_string, hash);
        }

        // Get audio IO layout
        let audio_io_layout = P::AUDIO_IO_LAYOUTS
            .first()
            .expect("Plugin must have at least one audio IO layout")
            .clone();

        // Generate port descriptors
        let port_descriptors = generate_port_descriptors::<P>();

        let num_inputs = audio_io_layout
            .main_input_channels
            .map(|n| n.get() as usize)
            .unwrap_or(0);
        let num_outputs = audio_io_layout
            .main_output_channels
            .map(|n| n.get() as usize)
            .unwrap_or(0);

        Box::new(Self {
            plugin: Arc::new(Mutex::new(plugin)),
            params,
            param_map,
            param_hash_map,
            param_by_port: HashMap::new(),
            port_descriptors,
            audio_inputs: vec![ptr::null_mut(); num_inputs],
            audio_outputs: vec![ptr::null_mut(); num_outputs],
            control_inputs: HashMap::new(),
            midi_input: None,
            midi_output: None,
            sample_rate,
            buffer_size: 512, // Default, will be updated
            is_activated: false,
            audio_io_layout,
            input_events: Vec::new(),
            output_events: Vec::new(),
            param_automation: ParamAutomationHandler::new(),
        })
    }

    /// Connect a port to a data location
    pub fn connect_port(&mut self, port: u32, data: *mut c_void) {
        if let Some(descriptor) = self.port_descriptors.iter().find(|p| p.index == port) {
            match descriptor.port_type {
                Lv2PortType::AudioInput => {
                    let input_index = self
                        .port_descriptors
                        .iter()
                        .filter(|p| p.port_type == Lv2PortType::AudioInput && p.index < port)
                        .count();
                    if input_index < self.audio_inputs.len() {
                        self.audio_inputs[input_index] = data as *mut f32;
                    }
                }
                Lv2PortType::AudioOutput => {
                    let output_index = self
                        .port_descriptors
                        .iter()
                        .filter(|p| p.port_type == Lv2PortType::AudioOutput && p.index < port)
                        .count();
                    if output_index < self.audio_outputs.len() {
                        self.audio_outputs[output_index] = data as *mut f32;
                    }
                }
                Lv2PortType::ControlInput => {
                    self.control_inputs.insert(port, data as *mut f32);
                }
                Lv2PortType::AtomInput => {
                    self.midi_input = Some(data);
                }
                Lv2PortType::AtomOutput => {
                    self.midi_output = Some(data);
                }
                _ => {}
            }
        }
    }

    /// Activate the plugin
    pub fn activate(&mut self) {
        if self.is_activated {
            return;
        }

        let buffer_config = BufferConfig {
            sample_rate: self.sample_rate as f32,
            min_buffer_size: None,
            max_buffer_size: self.buffer_size,
            process_mode: crate::prelude::ProcessMode::Realtime,
        };

        // Update parameter automation handler with sample rate
        self.param_automation.set_sample_rate(buffer_config.sample_rate);

        let mut plugin = self.plugin.lock();
        let mut context = Lv2InitContext::new();

        if plugin.initialize(&self.audio_io_layout, &buffer_config, &mut context) {
            plugin.reset();
            self.is_activated = true;
            
            // Update all parameter smoothers
            self.param_automation.update_all_smoothers(&self.params, &buffer_config);
        }
    }

    /// Deactivate the plugin
    pub fn deactivate(&mut self) {
        if !self.is_activated {
            return;
        }

        let mut plugin = self.plugin.lock();
        plugin.deactivate();
        self.is_activated = false;
    }

    /// Process audio
    pub fn run(&mut self, sample_count: u32) {
        if !self.is_activated {
            return;
        }

        // Update buffer size if changed
        if sample_count != self.buffer_size {
            self.buffer_size = sample_count;
        }

        // Read control port values and update parameters
        for (port_index, control_ptr) in &self.control_inputs {
            if let Some(param_id) = self.param_by_port.get(port_index) {
                if let Some(param_ptr) = self.param_map.get(param_id) {
                    unsafe {
                        if !control_ptr.is_null() {
                            let value = **control_ptr;
                            // Set the normalized parameter value using automation handler
                            self.param_automation.set_parameter_from_host(*param_ptr, value);
                        }
                    }
                }
            }
        }

        // Process MIDI input events
        self.input_events.clear();
        if let Some(midi_input) = self.midi_input {
            parse_atom_sequence::<P>(midi_input, &mut self.input_events);
        }

        // For now, implement a simple pass-through
        // TODO: Implement proper buffer management and call plugin.process()
        // This requires using the BufferManager from wrapper::util::buffer_management
        
        let num_samples = sample_count as usize;
        let copy_channels = self.audio_inputs.len().min(self.audio_outputs.len());
        
        unsafe {
            for i in 0..copy_channels {
                if !self.audio_inputs[i].is_null() && !self.audio_outputs[i].is_null() {
                    let input = std::slice::from_raw_parts(self.audio_inputs[i], num_samples);
                    let output = std::slice::from_raw_parts_mut(self.audio_outputs[i], num_samples);
                    output.copy_from_slice(input);
                }
            }
        }

        // Write output MIDI events to atom sequence
        if let Some(midi_output) = self.midi_output {
            // Assuming a reasonable buffer size for the atom sequence
            let atom_capacity = 4096;
            write_atom_sequence::<P>(&self.output_events, midi_output, atom_capacity);
        }
    }

    /// Get the LV2 descriptor for this plugin
    pub fn get_descriptor() -> &'static Lv2Descriptor {
        static mut DESCRIPTOR: Option<Lv2Descriptor> = None;

        unsafe {
            if DESCRIPTOR.is_none() {
                DESCRIPTOR = Some(Lv2Descriptor {
                    uri: P::LV2_URI.as_ptr() as *const c_char,
                    instantiate: Self::lv2_instantiate,
                    connect_port: Self::lv2_connect_port,
                    activate: Some(Self::lv2_activate),
                    run: Self::lv2_run,
                    deactivate: Some(Self::lv2_deactivate),
                    cleanup: Self::lv2_cleanup,
                    extension_data: Some(Self::lv2_extension_data),
                });
            }
            DESCRIPTOR.as_ref().unwrap()
        }
    }

    // LV2 C callback functions

    extern "C" fn lv2_instantiate(
        _descriptor: *const Lv2Descriptor,
        sample_rate: f64,
        _bundle_path: *const c_char,
        _features: *const *const Lv2Feature,
    ) -> Lv2Handle {
        let wrapper = Self::new(sample_rate);
        Box::into_raw(wrapper) as Lv2Handle
    }

    extern "C" fn lv2_connect_port(instance: Lv2Handle, port: u32, data: *mut c_void) {
        if instance.is_null() {
            return;
        }
        let wrapper = unsafe { &mut *(instance as *mut Self) };
        wrapper.connect_port(port, data);
    }

    extern "C" fn lv2_activate(instance: Lv2Handle) {
        if instance.is_null() {
            return;
        }
        let wrapper = unsafe { &mut *(instance as *mut Self) };
        wrapper.activate();
    }

    extern "C" fn lv2_run(instance: Lv2Handle, sample_count: u32) {
        if instance.is_null() {
            return;
        }
        let wrapper = unsafe { &mut *(instance as *mut Self) };
        wrapper.run(sample_count);
    }

    extern "C" fn lv2_deactivate(instance: Lv2Handle) {
        if instance.is_null() {
            return;
        }
        let wrapper = unsafe { &mut *(instance as *mut Self) };
        wrapper.deactivate();
    }

    extern "C" fn lv2_cleanup(instance: Lv2Handle) {
        if instance.is_null() {
            return;
        }
        unsafe {
            let _ = Box::from_raw(instance as *mut Self);
        }
    }

    extern "C" fn lv2_extension_data(uri: *const c_char) -> *const c_void {
        if uri.is_null() {
            return ptr::null();
        }

        unsafe {
            let uri_str = std::ffi::CStr::from_ptr(uri).to_str().unwrap_or("");

            // Check if the requested extension is the state interface
            // LV2_STATE__interface URI
            if uri_str == "http://lv2plug.in/ns/ext/state#interface" {
                static mut STATE_INTERFACE: Option<Lv2StateInterface> = None;
                if STATE_INTERFACE.is_none() {
                    STATE_INTERFACE = Some(create_state_interface::<Self>());
                }
                return STATE_INTERFACE.as_ref().unwrap() as *const Lv2StateInterface as *const c_void;
            }

            ptr::null()
        }
    }
}


impl<P: Plugin + Lv2Plugin> ParamChangeNotifier for Lv2Wrapper<P> {
    fn notify_param_value_changed(&self, _param_hash: u32, _normalized_value: f32) {
        // LV2 uses a port-based parameter system where the host reads parameter values
        // from control output ports. We don't need to actively notify the host.
        // The host will read the updated values from the control ports during processing.
        // TODO: If LV2 extensions for parameter change notifications are needed, implement them here
    }

    fn notify_begin_gesture(&self, _param_hash: u32) {
        // LV2 doesn't have a standard mechanism for gesture notifications
        // Some hosts may support this through custom extensions
        // TODO: Implement LV2-specific gesture notification if needed
    }

    fn notify_end_gesture(&self, _param_hash: u32) {
        // LV2 doesn't have a standard mechanism for gesture notifications
        // Some hosts may support this through custom extensions
        // TODO: Implement LV2-specific gesture notification if needed
    }
}

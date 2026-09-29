//! The main VST2 wrapper implementation.

use std::ffi::c_void;
use std::os::raw::c_char;
use std::ptr;
use std::sync::Arc;

use atomic_refcell::AtomicRefCell;
use crossbeam::atomic::AtomicCell;
use parking_lot::Mutex;
use std::collections::VecDeque;

use crate::params::internals::ParamPtr;
use crate::prelude::{AudioIOLayout, BufferConfig, NoteEvent, Params, Plugin, PluginNoteEvent};
use crate::plugin::vst2::Vst2Plugin;
use crate::wrapper::param_automation::{ParamAutomationHandler, ParamChangeNotifier};

// VST2 opcodes for the dispatcher
const EFFECT_OPCODE_OPEN: i32 = 0;
const EFFECT_OPCODE_CLOSE: i32 = 1;
const EFFECT_OPCODE_SET_PROGRAM: i32 = 2;
const EFFECT_OPCODE_GET_PROGRAM: i32 = 3;
const EFFECT_OPCODE_SET_PROGRAM_NAME: i32 = 4;
const EFFECT_OPCODE_GET_PROGRAM_NAME: i32 = 5;
const EFFECT_OPCODE_GET_PARAM_LABEL: i32 = 6;
const EFFECT_OPCODE_GET_PARAM_DISPLAY: i32 = 7;
const EFFECT_OPCODE_GET_PARAM_NAME: i32 = 8;
const EFFECT_OPCODE_SET_SAMPLE_RATE: i32 = 10;
const EFFECT_OPCODE_SET_BLOCK_SIZE: i32 = 11;
const EFFECT_OPCODE_MAIN_CHANGED: i32 = 12;
const EFFECT_OPCODE_EDIT_GET_RECT: i32 = 13;
const EFFECT_OPCODE_EDIT_OPEN: i32 = 14;
const EFFECT_OPCODE_EDIT_CLOSE: i32 = 15;
const EFFECT_OPCODE_IDENTIFY: i32 = 22;
const EFFECT_OPCODE_GET_CHUNK: i32 = 23;
const EFFECT_OPCODE_SET_CHUNK: i32 = 24;
const EFFECT_OPCODE_PROCESS_EVENTS: i32 = 25;
const EFFECT_OPCODE_CAN_BE_AUTOMATED: i32 = 26;
const EFFECT_OPCODE_GET_PROGRAM_NAME_INDEXED: i32 = 29;
const EFFECT_OPCODE_GET_INPUT_PROPERTIES: i32 = 33;
const EFFECT_OPCODE_GET_OUTPUT_PROPERTIES: i32 = 34;
const EFFECT_OPCODE_GET_PLUGIN_CATEGORY: i32 = 35;
const EFFECT_OPCODE_GET_EFFECT_NAME: i32 = 45;
const EFFECT_OPCODE_GET_VENDOR_STRING: i32 = 47;
const EFFECT_OPCODE_GET_PRODUCT_STRING: i32 = 48;
const EFFECT_OPCODE_GET_VENDOR_VERSION: i32 = 49;
const EFFECT_OPCODE_CAN_DO: i32 = 51;
const EFFECT_OPCODE_GET_VST_VERSION: i32 = 58;

// VST2 flags
const EFFECT_FLAGS_HAS_EDITOR: i32 = 1 << 0;
const EFFECT_FLAGS_CAN_REPLACING: i32 = 1 << 4;
const EFFECT_FLAGS_PROGRAM_CHUNKS: i32 = 1 << 5;
const EFFECT_FLAGS_IS_SYNTH: i32 = 1 << 8;
const EFFECT_FLAGS_CAN_DOUBLE_REPLACING: i32 = 1 << 12;

// VST2 magic number
const VST2_MAGIC: i32 = 0x56737450; // 'VstP'

// VST2 MIDI event types
const VST_EVENT_MIDI: i32 = 1;
const VST_EVENT_SYSEX: i32 = 6;

/// VST2 MIDI event structure
#[repr(C)]
pub struct VstMidiEvent {
    pub event_type: i32,
    pub byte_size: i32,
    pub delta_frames: i32,
    pub flags: i32,
    pub note_length: i32,
    pub note_offset: i32,
    pub midi_data: [u8; 4],
    pub detune: i8,
    pub note_off_velocity: i8,
    pub reserved1: i8,
    pub reserved2: i8,
}

/// VST2 SysEx event structure
#[repr(C)]
pub struct VstSysExEvent {
    pub event_type: i32,
    pub byte_size: i32,
    pub delta_frames: i32,
    pub flags: i32,
    pub data_size: i32,
    pub reserved1: isize,
    pub system_data: *const u8,
    pub reserved2: isize,
}

// Note: We don't actually use a union here since it's complex to handle in Rust
// Instead, we'll cast the VstEvent pointer to the appropriate type when needed

/// VST2 event structure
#[repr(C)]
pub struct VstEvent {
    pub event_type: i32,
    pub byte_size: i32,
    pub delta_frames: i32,
    pub flags: i32,
    pub data: [u8; 16],
}

/// VST2 events structure
#[repr(C)]
pub struct VstEvents {
    pub num_events: i32,
    pub reserved: isize,
    pub events: [*mut VstEvent; 2],
}

/// The VST2 AEffect structure
#[repr(C)]
pub struct AEffect {
    pub magic: i32,
    pub dispatcher: extern "C" fn(
        effect: *mut AEffect,
        opcode: i32,
        index: i32,
        value: isize,
        ptr: *mut c_void,
        opt: f32,
    ) -> isize,
    pub process: extern "C" fn(effect: *mut AEffect, inputs: *mut *mut f32, outputs: *mut *mut f32, sample_frames: i32),
    pub set_parameter: extern "C" fn(effect: *mut AEffect, index: i32, value: f32),
    pub get_parameter: extern "C" fn(effect: *mut AEffect, index: i32) -> f32,
    pub num_programs: i32,
    pub num_params: i32,
    pub num_inputs: i32,
    pub num_outputs: i32,
    pub flags: i32,
    pub reserved1: isize,
    pub reserved2: isize,
    pub initial_delay: i32,
    pub real_qualities: i32,
    pub off_qualities: i32,
    pub io_ratio: f32,
    pub object: *mut c_void,
    pub user: *mut c_void,
    pub unique_id: i32,
    pub version: i32,
    pub process_replacing: extern "C" fn(effect: *mut AEffect, inputs: *mut *mut f32, outputs: *mut *mut f32, sample_frames: i32),
    pub process_double_replacing: extern "C" fn(effect: *mut AEffect, inputs: *mut *mut f64, outputs: *mut *mut f64, sample_frames: i32),
    pub future: [u8; 56],
}

/// Host callback function type
pub type HostCallbackProc = extern "C" fn(
    effect: *mut AEffect,
    opcode: i32,
    index: i32,
    value: isize,
    ptr: *mut c_void,
    opt: f32,
) -> isize;

/// This struct implements the VST2 plugin interface and translates calls
/// to the NIH-plug Plugin trait.
pub struct Vst2Wrapper<P: Plugin> {
    /// The wrapped plugin instance
    plugin: Mutex<P>,
    /// The plugin's parameters
    params: Arc<dyn Params>,
    /// Parameter pointers by index
    param_by_index: Vec<ParamPtr>,
    /// Parameter IDs by index for looking up parameter info
    param_id_by_index: Vec<String>,
    /// Parameter hash by index for automation notifications
    param_hash_by_index: Vec<u32>,
    /// Current buffer configuration
    current_buffer_config: AtomicCell<Option<BufferConfig>>,
    /// Current audio IO layout
    current_audio_io_layout: AtomicCell<AudioIOLayout>,
    /// The host callback function
    host_callback: HostCallbackProc,
    /// The AEffect structure that gets returned to the host
    effect: *mut AEffect,
    /// Incoming MIDI events for the current processing cycle
    input_events: AtomicRefCell<VecDeque<PluginNoteEvent<P>>>,
    /// Unified parameter automation handler
    param_automation: ParamAutomationHandler,
}

impl<P: Plugin + Vst2Plugin> Vst2Wrapper<P> {
    /// Create a new VST2 wrapper instance and return the AEffect pointer.
    pub fn new(host_callback: HostCallbackProc) -> *mut AEffect {
        let plugin = P::default();
        let params = plugin.params();
        
        // Collect all parameters
        let param_map: Vec<_> = params.param_map().into_iter().collect();
        let param_by_index: Vec<ParamPtr> = param_map.iter().map(|(_, ptr, _)| *ptr).collect();
        let param_id_by_index: Vec<String> = param_map.iter().map(|(id, _, _)| id.clone()).collect();
        let param_hash_by_index: Vec<u32> = param_id_by_index
            .iter()
            .map(|id| crate::wrapper::util::hash_param_id(id))
            .collect();
        
        // Get the default audio IO layout
        let audio_io_layouts = P::AUDIO_IO_LAYOUTS;
        let current_audio_io_layout = audio_io_layouts
            .first()
            .copied()
            .unwrap_or_default();
        
        // Create the AEffect structure
        let mut effect = Box::new(AEffect {
            magic: VST2_MAGIC,
            dispatcher: Self::dispatcher_callback,
            process: Self::process_callback,
            set_parameter: Self::set_parameter_callback,
            get_parameter: Self::get_parameter_callback,
            num_programs: 1,
            num_params: param_by_index.len() as i32,
            num_inputs: current_audio_io_layout.main_input_channels.map(|c| c.get() as i32).unwrap_or(0),
            num_outputs: current_audio_io_layout.main_output_channels.map(|c| c.get() as i32).unwrap_or(0),
            flags: EFFECT_FLAGS_CAN_REPLACING | EFFECT_FLAGS_PROGRAM_CHUNKS,
            reserved1: 0,
            reserved2: 0,
            initial_delay: 0,
            real_qualities: 0,
            off_qualities: 0,
            io_ratio: 1.0,
            object: ptr::null_mut(),
            user: ptr::null_mut(),
            unique_id: P::VST2_UNIQUE_ID,
            version: P::VERSION.parse::<i32>().unwrap_or(1000),
            process_replacing: Self::process_replacing_callback,
            process_double_replacing: Self::process_double_replacing_callback,
            future: [0; 56],
        });
        
        let effect_ptr = Box::into_raw(effect);
        
        // Create the wrapper
        let wrapper = Box::new(Self {
            plugin: Mutex::new(plugin),
            params,
            param_by_index,
            param_id_by_index,
            param_hash_by_index,
            current_buffer_config: AtomicCell::new(None),
            current_audio_io_layout: AtomicCell::new(current_audio_io_layout),
            host_callback,
            effect: effect_ptr,
            input_events: AtomicRefCell::new(VecDeque::new()),
            param_automation: ParamAutomationHandler::new(),
        });
        
        // Store the wrapper pointer in the AEffect's object field
        unsafe {
            (*effect_ptr).object = Box::into_raw(wrapper) as *mut c_void;
        }
        
        effect_ptr
    }
    
    /// Get the wrapper from an AEffect pointer
    unsafe fn from_effect(effect: *mut AEffect) -> &'static mut Self {
        &mut *((*effect).object as *mut Self)
    }
    
    /// VST2 dispatcher callback
    extern "C" fn dispatcher_callback(
        effect: *mut AEffect,
        opcode: i32,
        index: i32,
        value: isize,
        ptr: *mut c_void,
        opt: f32,
    ) -> isize {
        if effect.is_null() {
            return 0;
        }
        
        let wrapper = unsafe { Self::from_effect(effect) };
        wrapper.dispatcher(opcode, index, value, ptr, opt)
    }
    
    /// Handle dispatcher opcodes
    fn dispatcher(
        &mut self,
        opcode: i32,
        index: i32,
        _value: isize,
        ptr: *mut c_void,
        opt: f32,
    ) -> isize {
        match opcode {
            EFFECT_OPCODE_OPEN => {
                // Plugin is being opened
                0
            }
            EFFECT_OPCODE_CLOSE => {
                // Plugin is being closed
                0
            }
            EFFECT_OPCODE_SET_SAMPLE_RATE => {
                // Update buffer config with new sample rate
                let sample_rate = opt;
                let mut config = self.current_buffer_config.load().unwrap_or(BufferConfig {
                    sample_rate,
                    min_buffer_size: None,
                    max_buffer_size: 8192,
                    process_mode: crate::prelude::ProcessMode::Realtime,
                });
                config.sample_rate = sample_rate;
                self.current_buffer_config.store(Some(config));
                
                // Update parameter automation handler with new sample rate
                self.param_automation.set_sample_rate(sample_rate);
                
                // Initialize the plugin if we have both sample rate and block size
                if config.max_buffer_size > 0 {
                    // TODO: Implement proper InitContext for VST2
                    // For now, we skip initialization here and do it in MAIN_CHANGED
                    
                    // Update all parameter smoothers
                    self.param_automation.update_all_smoothers(&self.params, &config);
                }
                0
            }
            EFFECT_OPCODE_SET_BLOCK_SIZE => {
                // Update buffer config with new block size
                let block_size = _value as u32;
                let mut config = self.current_buffer_config.load().unwrap_or(BufferConfig {
                    sample_rate: 44100.0,
                    min_buffer_size: None,
                    max_buffer_size: block_size,
                    process_mode: crate::prelude::ProcessMode::Realtime,
                });
                config.max_buffer_size = block_size;
                self.current_buffer_config.store(Some(config));
                
                // Initialize the plugin if we have both sample rate and block size
                if config.sample_rate > 0.0 {
                    // TODO: Implement proper InitContext for VST2
                    // For now, we skip initialization here and do it in MAIN_CHANGED
                }
                0
            }
            EFFECT_OPCODE_MAIN_CHANGED => {
                // Processing state changed
                if _value == 1 {
                    // Start processing
                    let mut plugin = self.plugin.lock();
                    plugin.reset();
                } else {
                    // Stop processing
                    let mut plugin = self.plugin.lock();
                    plugin.deactivate();
                }
                0
            }
            EFFECT_OPCODE_GET_PARAM_NAME => {
                if index >= 0 && (index as usize) < self.param_by_index.len() {
                    let param_id = &self.param_id_by_index[index as usize];
                    if !ptr.is_null() {
                        let name_ptr = ptr as *mut c_char;
                        let name_bytes = param_id.as_bytes();
                        let copy_len = name_bytes.len().min(23); // VST2 param names are max 24 chars
                        unsafe {
                            ptr::copy_nonoverlapping(name_bytes.as_ptr(), name_ptr as *mut u8, copy_len);
                            *name_ptr.add(copy_len) = 0; // Null terminate
                        }
                    }
                }
                0
            }
            EFFECT_OPCODE_GET_PARAM_LABEL => {
                if index >= 0 && (index as usize) < self.param_by_index.len() {
                    let param_ptr = self.param_by_index[index as usize];
                    let unit = unsafe { param_ptr.unit() };
                    if !ptr.is_null() {
                        let label_ptr = ptr as *mut c_char;
                        let unit_bytes = unit.as_bytes();
                        let copy_len = unit_bytes.len().min(7); // VST2 param labels are max 8 chars
                        unsafe {
                            ptr::copy_nonoverlapping(unit_bytes.as_ptr(), label_ptr as *mut u8, copy_len);
                            *label_ptr.add(copy_len) = 0; // Null terminate
                        }
                    }
                }
                0
            }
            EFFECT_OPCODE_GET_PARAM_DISPLAY => {
                if index >= 0 && (index as usize) < self.param_by_index.len() {
                    let param_ptr = self.param_by_index[index as usize];
                    let normalized = unsafe { param_ptr.modulated_normalized_value() };
                    let display = unsafe { param_ptr.normalized_value_to_string(normalized, false) };
                    if !ptr.is_null() {
                        let display_ptr = ptr as *mut c_char;
                        let display_bytes = display.as_bytes();
                        let copy_len = display_bytes.len().min(23); // VST2 param displays are max 24 chars
                        unsafe {
                            ptr::copy_nonoverlapping(display_bytes.as_ptr(), display_ptr as *mut u8, copy_len);
                            *display_ptr.add(copy_len) = 0; // Null terminate
                        }
                    }
                }
                0
            }
            EFFECT_OPCODE_GET_EFFECT_NAME => {
                if !ptr.is_null() {
                    let name = P::NAME;
                    let name_ptr = ptr as *mut c_char;
                    let name_bytes = name.as_bytes();
                    let copy_len = name_bytes.len().min(31); // VST2 effect names are max 32 chars
                    unsafe {
                        ptr::copy_nonoverlapping(name_bytes.as_ptr(), name_ptr as *mut u8, copy_len);
                        *name_ptr.add(copy_len) = 0; // Null terminate
                    }
                }
                0
            }
            EFFECT_OPCODE_GET_VENDOR_STRING => {
                if !ptr.is_null() {
                    let vendor = P::VENDOR;
                    let vendor_ptr = ptr as *mut c_char;
                    let vendor_bytes = vendor.as_bytes();
                    let copy_len = vendor_bytes.len().min(63); // VST2 vendor strings are max 64 chars
                    unsafe {
                        ptr::copy_nonoverlapping(vendor_bytes.as_ptr(), vendor_ptr as *mut u8, copy_len);
                        *vendor_ptr.add(copy_len) = 0; // Null terminate
                    }
                }
                0
            }
            EFFECT_OPCODE_GET_PRODUCT_STRING => {
                if !ptr.is_null() {
                    let product = P::NAME;
                    let product_ptr = ptr as *mut c_char;
                    let product_bytes = product.as_bytes();
                    let copy_len = product_bytes.len().min(63); // VST2 product strings are max 64 chars
                    unsafe {
                        ptr::copy_nonoverlapping(product_bytes.as_ptr(), product_ptr as *mut u8, copy_len);
                        *product_ptr.add(copy_len) = 0; // Null terminate
                    }
                }
                0
            }
            EFFECT_OPCODE_GET_VENDOR_VERSION => {
                P::VERSION.parse::<isize>().unwrap_or(1000)
            }
            EFFECT_OPCODE_GET_PLUGIN_CATEGORY => {
                P::VST2_CATEGORY.as_vst2_constant() as isize
            }
            EFFECT_OPCODE_IDENTIFY => {
                // Return VST2 magic number
                VST2_MAGIC as isize
            }
            EFFECT_OPCODE_CAN_BE_AUTOMATED => {
                // All parameters can be automated
                1
            }
            EFFECT_OPCODE_GET_VST_VERSION => {
                2400 // VST 2.4
            }
            EFFECT_OPCODE_PROCESS_EVENTS => {
                // Process incoming MIDI events
                if !ptr.is_null() {
                    self.process_vst_events(ptr as *const VstEvents);
                }
                1
            }
            _ => 0,
        }
    }
    
    /// Process VST2 events and translate them to NIH-plug format
    fn process_vst_events(&mut self, events: *const VstEvents) {
        if events.is_null() {
            return;
        }
        
        let events = unsafe { &*events };
        let num_events = events.num_events as usize;
        
        // Clear previous events
        self.input_events.borrow_mut().clear();
        
        for i in 0..num_events.min(2) {
            let event_ptr = unsafe { *events.events.get_unchecked(i) };
            if event_ptr.is_null() {
                continue;
            }
            
            let event = unsafe { &*event_ptr };
            
            match event.event_type {
                VST_EVENT_MIDI => {
                    // Cast to VstMidiEvent
                    let midi_event = unsafe { &*(event_ptr as *const VstMidiEvent) };
                    self.translate_midi_event(midi_event);
                }
                VST_EVENT_SYSEX => {
                    // SysEx events - not implemented yet
                    // TODO: Implement SysEx translation
                }
                _ => {}
            }
        }
    }
    
    /// Translate a VST2 MIDI event to NIH-plug format
    fn translate_midi_event(&mut self, event: &VstMidiEvent) {
        let status = event.midi_data[0];
        let data1 = event.midi_data[1];
        let data2 = event.midi_data[2];
        let timing = event.delta_frames as u32;
        
        // Extract channel and message type
        let channel = status & 0x0F;
        let message_type = status & 0xF0;
        
        let note_event = match message_type {
            0x80 => {
                // Note Off
                Some(NoteEvent::NoteOff {
                    timing,
                    voice_id: None,
                    channel,
                    note: data1,
                    velocity: (data2 as f32) / 127.0,
                })
            }
            0x90 => {
                // Note On (velocity 0 is Note Off)
                if data2 == 0 {
                    Some(NoteEvent::NoteOff {
                        timing,
                        voice_id: None,
                        channel,
                        note: data1,
                        velocity: 0.0,
                    })
                } else {
                    Some(NoteEvent::NoteOn {
                        timing,
                        voice_id: None,
                        channel,
                        note: data1,
                        velocity: (data2 as f32) / 127.0,
                    })
                }
            }
            0xA0 => {
                // Polyphonic Aftertouch
                Some(NoteEvent::PolyPressure {
                    timing,
                    voice_id: None,
                    channel,
                    note: data1,
                    pressure: (data2 as f32) / 127.0,
                })
            }
            0xB0 => {
                // Control Change
                Some(NoteEvent::MidiCC {
                    timing,
                    channel,
                    cc: data1,
                    value: (data2 as f32) / 127.0,
                })
            }
            0xC0 => {
                // Program Change
                Some(NoteEvent::MidiProgramChange {
                    timing,
                    channel,
                    program: data1,
                })
            }
            0xD0 => {
                // Channel Aftertouch
                Some(NoteEvent::MidiChannelPressure {
                    timing,
                    channel,
                    pressure: (data1 as f32) / 127.0,
                })
            }
            0xE0 => {
                // Pitch Bend
                let value = ((data2 as u16) << 7) | (data1 as u16);
                let normalized = ((value as f32) / 8192.0) - 1.0; // -1.0 to 1.0
                Some(NoteEvent::MidiPitchBend {
                    timing,
                    channel,
                    value: normalized,
                })
            }
            _ => None,
        };
        
        if let Some(event) = note_event {
            self.input_events.borrow_mut().push_back(PluginNoteEvent::<P>::from(event));
        }
    }
    
    /// Set parameter callback
    extern "C" fn set_parameter_callback(effect: *mut AEffect, index: i32, value: f32) {
        if effect.is_null() {
            return;
        }
        
        let wrapper = unsafe { Self::from_effect(effect) };
        wrapper.set_parameter(index, value);
    }
    
    /// Set a parameter value (normalized 0.0-1.0)
    fn set_parameter(&mut self, index: i32, value: f32) {
        if index >= 0 && (index as usize) < self.param_by_index.len() {
            let param_ptr = self.param_by_index[index as usize];
            unsafe {
                self.param_automation.set_parameter_from_host(param_ptr, value);
            }
        }
    }
    
    /// Get parameter callback
    extern "C" fn get_parameter_callback(effect: *mut AEffect, index: i32) -> f32 {
        if effect.is_null() {
            return 0.0;
        }
        
        let wrapper = unsafe { Self::from_effect(effect) };
        wrapper.get_parameter(index)
    }
    
    /// Get a parameter value (normalized 0.0-1.0)
    fn get_parameter(&self, index: i32) -> f32 {
        if index >= 0 && (index as usize) < self.param_by_index.len() {
            let param_ptr = self.param_by_index[index as usize];
            unsafe { param_ptr.modulated_normalized_value() }
        } else {
            0.0
        }
    }
    
    /// Process callback (deprecated, but some hosts still use it)
    extern "C" fn process_callback(
        effect: *mut AEffect,
        inputs: *mut *mut f32,
        outputs: *mut *mut f32,
        sample_frames: i32,
    ) {
        // Just call process_replacing
        Self::process_replacing_callback(effect, inputs, outputs, sample_frames);
    }
    
    /// Process replacing callback (main audio processing)
    extern "C" fn process_replacing_callback(
        effect: *mut AEffect,
        inputs: *mut *mut f32,
        outputs: *mut *mut f32,
        sample_frames: i32,
    ) {
        if effect.is_null() || sample_frames <= 0 {
            return;
        }
        
        let wrapper = unsafe { Self::from_effect(effect) };
        wrapper.process_replacing(inputs, outputs, sample_frames as usize);
    }
    
    /// Process audio with f32 samples
    fn process_replacing(
        &mut self,
        inputs: *mut *mut f32,
        outputs: *mut *mut f32,
        sample_frames: usize,
    ) {
        let config = match self.current_buffer_config.load() {
            Some(config) => config,
            None => return,
        };
        
        let layout = self.current_audio_io_layout.load();
        let num_inputs = layout.main_input_channels.map(|c| c.get()).unwrap_or(0) as usize;
        let num_outputs = layout.main_output_channels.map(|c| c.get()).unwrap_or(0) as usize;
        
        // Create input and output slices
        let input_slices: Vec<&[f32]> = (0..num_inputs)
            .map(|i| unsafe {
                std::slice::from_raw_parts(*inputs.add(i), sample_frames)
            })
            .collect();
        
        let mut output_slices: Vec<&mut [f32]> = (0..num_outputs)
            .map(|i| unsafe {
                std::slice::from_raw_parts_mut(*outputs.add(i), sample_frames)
            })
            .collect();
        
        // Create a buffer for processing
        // For now, we'll do a simple pass-through and copy input to output
        // TODO: Implement proper buffer management and call plugin.process()
        for (input, output) in input_slices.iter().zip(output_slices.iter_mut()) {
            output.copy_from_slice(input);
        }
    }
    
    /// Process double replacing callback (64-bit audio processing)
    extern "C" fn process_double_replacing_callback(
        effect: *mut AEffect,
        inputs: *mut *mut f64,
        outputs: *mut *mut f64,
        sample_frames: i32,
    ) {
        if effect.is_null() || sample_frames <= 0 {
            return;
        }
        
        // For now, just zero the outputs
        // TODO: Implement proper f64 processing
        let wrapper = unsafe { Self::from_effect(effect) };
        let layout = wrapper.current_audio_io_layout.load();
        let num_outputs = layout.main_output_channels.map(|c| c.get()).unwrap_or(0) as usize;
        
        for i in 0..num_outputs {
            unsafe {
                let output = std::slice::from_raw_parts_mut(*outputs.add(i), sample_frames as usize);
                output.fill(0.0);
            }
        }
    }
}


// VST2 host callback opcodes for parameter automation
const HOST_OPCODE_AUTOMATE: i32 = 0;
const HOST_OPCODE_BEGIN_EDIT: i32 = 43;
const HOST_OPCODE_END_EDIT: i32 = 44;

impl<P: Plugin + Vst2Plugin> ParamChangeNotifier for Vst2Wrapper<P> {
    fn notify_param_value_changed(&self, param_hash: u32, normalized_value: f32) {
        // Find the parameter index from the hash
        if let Some(index) = self.param_hash_by_index.iter().position(|&h| h == param_hash) {
            // Call the host's automate callback
            unsafe {
                (self.host_callback)(
                    self.effect,
                    HOST_OPCODE_AUTOMATE,
                    index as i32,
                    0,
                    ptr::null_mut(),
                    normalized_value,
                );
            }
        }
    }

    fn notify_begin_gesture(&self, param_hash: u32) {
        // Find the parameter index from the hash
        if let Some(index) = self.param_hash_by_index.iter().position(|&h| h == param_hash) {
            // Call the host's begin edit callback
            unsafe {
                (self.host_callback)(
                    self.effect,
                    HOST_OPCODE_BEGIN_EDIT,
                    index as i32,
                    0,
                    ptr::null_mut(),
                    0.0,
                );
            }
        }
    }

    fn notify_end_gesture(&self, param_hash: u32) {
        // Find the parameter index from the hash
        if let Some(index) = self.param_hash_by_index.iter().position(|&h| h == param_hash) {
            // Call the host's end edit callback
            unsafe {
                (self.host_callback)(
                    self.effect,
                    HOST_OPCODE_END_EDIT,
                    index as i32,
                    0,
                    ptr::null_mut(),
                    0.0,
                );
            }
        }
    }
}

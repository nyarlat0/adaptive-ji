//! Audio Units wrapper implementation.
//!
//! This module contains the main AU wrapper struct that translates between
//! the AU API and NIH-plug's Plugin trait.

use std::sync::Arc;
use atomic_refcell::AtomicRefCell;
use crossbeam::atomic::AtomicCell;
use parking_lot::Mutex;
use std::collections::VecDeque;

use crate::params::internals::ParamPtr;
use crate::prelude::{
    AudioIOLayout, BufferConfig, NoteEvent, 
    Params, Plugin, PluginNoteEvent, ProcessMode, ProcessStatus,
};
use crate::plugin::au::AuPlugin;
use crate::wrapper::param_automation::{ParamAutomationHandler, ParamChangeNotifier};

/// The main AU wrapper struct.
///
/// This struct implements the AU component interface and translates calls
/// to the NIH-plug Plugin trait.
pub struct AuWrapper<P: Plugin> {
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
    /// Incoming MIDI events for the current processing cycle
    input_events: AtomicRefCell<VecDeque<PluginNoteEvent<P>>>,
    /// Whether the plugin has been initialized
    is_initialized: AtomicCell<bool>,
    /// Unified parameter automation handler
    param_automation: ParamAutomationHandler,
}

impl<P: Plugin + AuPlugin> AuWrapper<P> {
    /// Create a new AU wrapper instance.
    pub fn new() -> Self {
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
        
        Self {
            plugin: Mutex::new(plugin),
            params,
            param_by_index,
            param_id_by_index,
            param_hash_by_index,
            current_buffer_config: AtomicCell::new(None),
            current_audio_io_layout: AtomicCell::new(current_audio_io_layout),
            input_events: AtomicRefCell::new(VecDeque::new()),
            is_initialized: AtomicCell::new(false),
            param_automation: ParamAutomationHandler::new(),
        }
    }
    
    /// Initialize the plugin with the given audio configuration.
    pub fn initialize(&mut self, sample_rate: f64, max_frames_per_slice: u32) -> bool {
        let config = BufferConfig {
            sample_rate: sample_rate as f32,
            min_buffer_size: None,
            max_buffer_size: max_frames_per_slice,
            process_mode: ProcessMode::Realtime,
        };
        
        self.current_buffer_config.store(Some(config));
        
        // Update parameter automation handler with new sample rate
        self.param_automation.set_sample_rate(sample_rate as f32);
        
        let mut plugin = self.plugin.lock();
        let layout = self.current_audio_io_layout.load();
        
        // TODO: Implement proper InitContext for AU
        // For now, we skip initialization
        let success = true;
        
        if success {
            plugin.reset();
            self.is_initialized.store(true);
            
            // Update all parameter smoothers
            self.param_automation.update_all_smoothers(&self.params, &config);
        }
        
        success
    }
    
    /// Reset the plugin state.
    pub fn reset(&mut self) {
        let mut plugin = self.plugin.lock();
        plugin.reset();
    }
    
    /// Deactivate the plugin.
    pub fn deactivate(&mut self) {
        let mut plugin = self.plugin.lock();
        plugin.deactivate();
        self.is_initialized.store(false);
    }
    
    /// Get the number of parameters.
    pub fn param_count(&self) -> usize {
        self.param_by_index.len()
    }
    
    /// Get a parameter value (normalized 0.0-1.0).
    pub fn get_parameter(&self, index: usize) -> f32 {
        if index < self.param_by_index.len() {
            let param_ptr = self.param_by_index[index];
            unsafe { param_ptr.normalized_value() }
        } else {
            0.0
        }
    }
    
    /// Set a parameter value (normalized 0.0-1.0).
    pub fn set_parameter(&mut self, index: usize, value: f32) {
        if index < self.param_by_index.len() {
            let param_ptr = self.param_by_index[index];
            unsafe {
                self.param_automation.set_parameter_from_host(param_ptr, value);
            }
        }
    }
    
    /// Get parameter info by index.
    pub fn get_parameter_info(&self, index: usize) -> Option<ParameterInfo> {
        if index < self.param_by_index.len() {
            let param_ptr = self.param_by_index[index];
            let param_id = &self.param_id_by_index[index];
            
            unsafe {
                Some(ParameterInfo {
                    id: param_id.clone(),
                    name: param_ptr.name().to_string(),
                    unit: param_ptr.unit().to_string(),
                    min_value: 0.0,
                    max_value: 1.0,
                    default_value: param_ptr.default_normalized_value(),
                })
            }
        } else {
            None
        }
    }
    
    /// Process MIDI events and add them to the input queue.
    pub fn process_midi_event(&mut self, status: u8, data1: u8, data2: u8, timing: u32) {
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
            self.input_events.borrow_mut().push_back(PluginNoteEvent::from(event));
        }
    }
    
    /// Render audio using AU's pull-based model.
    ///
    /// This is the main audio processing function for AU plugins.
    /// AU uses a pull-based model where the plugin is asked to render
    /// a specific number of frames into provided buffers.
    pub fn render(
        &mut self,
        input_buffers: &[&[f32]],
        output_buffers: &mut [&mut [f32]],
        num_frames: usize,
    ) -> ProcessStatus {
        if !self.is_initialized.load() {
            // If not initialized, just output silence
            for output in output_buffers.iter_mut() {
                output[..num_frames].fill(0.0);
            }
            return ProcessStatus::Normal;
        }
        
        // For now, implement a simple pass-through
        // TODO: Implement proper buffer management and call plugin.process()
        let copy_channels = input_buffers.len().min(output_buffers.len());
        for i in 0..copy_channels {
            let copy_len = num_frames.min(input_buffers[i].len()).min(output_buffers[i].len());
            output_buffers[i][..copy_len].copy_from_slice(&input_buffers[i][..copy_len]);
        }
        
        // Clear any remaining output channels
        for output in output_buffers.iter_mut().skip(copy_channels) {
            output[..num_frames].fill(0.0);
        }
        
        ProcessStatus::Normal
    }
    
    /// Get the current latency in samples.
    pub fn get_latency(&self) -> u32 {
        // TODO: Get actual latency from plugin
        0
    }
    
    /// Get the tail time in samples.
    pub fn get_tail_time(&self) -> Option<u32> {
        // TODO: Get actual tail time from plugin
        None
    }
    
    /// Save the plugin state as a preset.
    ///
    /// Returns the serialized state data that can be stored by the AU host.
    pub fn save_preset(&self) -> Result<Vec<u8>, String> {
        use crate::wrapper::state::PluginState;
        
        let state = PluginState::from_plugin(&*self.plugin.lock(), self.params.clone());
        
        // Serialize to JSON
        serde_json::to_vec(&state)
            .map_err(|e| format!("Failed to serialize preset: {}", e))
    }
    
    /// Load a preset from serialized state data.
    ///
    /// This restores the plugin state from data previously saved with `save_preset`.
    pub fn load_preset(&mut self, data: &[u8]) -> Result<(), String> {
        use crate::wrapper::state::PluginState;
        
        // Deserialize from JSON
        let mut state: PluginState = serde_json::from_slice(data)
            .map_err(|e| format!("Failed to deserialize preset: {}", e))?;
        
        // Apply any state filters
        P::filter_state(&mut state);
        
        // Load the state into the plugin
        let mut plugin = self.plugin.lock();
        state.load_into_plugin(&mut *plugin, self.params.clone());
        
        Ok(())
    }
    
    /// Get the plugin state as raw bytes for AU property-based state management.
    pub fn get_state(&self) -> Result<Vec<u8>, String> {
        self.save_preset()
    }
    
    /// Set the plugin state from raw bytes for AU property-based state management.
    pub fn set_state(&mut self, data: &[u8]) -> Result<(), String> {
        self.load_preset(data)
    }
}

/// Parameter information structure.
#[derive(Debug, Clone)]
pub struct ParameterInfo {
    pub id: String,
    pub name: String,
    pub unit: String,
    pub min_value: f32,
    pub max_value: f32,
    pub default_value: f32,
}


impl<P: Plugin + AuPlugin> ParamChangeNotifier for AuWrapper<P> {
    fn notify_param_value_changed(&self, _param_hash: u32, _normalized_value: f32) {
        // AU parameter changes are typically handled through the AU parameter system
        // The host polls parameter values, so we don't need to actively notify
        // In a full implementation, this would call AudioUnitSetParameter or similar
        // TODO: Implement AU-specific parameter change notification
    }

    fn notify_begin_gesture(&self, _param_hash: u32) {
        // AU doesn't have a direct equivalent to begin gesture
        // Some hosts may support this through custom extensions
        // TODO: Implement AU-specific gesture notification if needed
    }

    fn notify_end_gesture(&self, _param_hash: u32) {
        // AU doesn't have a direct equivalent to end gesture
        // Some hosts may support this through custom extensions
        // TODO: Implement AU-specific gesture notification if needed
    }
}

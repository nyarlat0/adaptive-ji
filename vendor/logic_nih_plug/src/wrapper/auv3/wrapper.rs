//! Audio Units v3 (AUv3) wrapper implementation.
//!
//! This module contains the main AUv3 wrapper struct that translates between
//! the AUv3 API and NIH-plug's Plugin trait. AUv3 uses app extensions and
//! a modern architecture compared to the original AU format.

use std::sync::Arc;
use atomic_refcell::AtomicRefCell;
use crossbeam::atomic::AtomicCell;
use parking_lot::Mutex;
use std::collections::VecDeque;

use crate::prelude::{
    AudioIOLayout, AuxiliaryBuffers, Buffer, BufferConfig, MidiConfig, NoteEvent, 
    ParamPtr, Params, Plugin, PluginNoteEvent, ProcessMode, ProcessStatus,
};
use crate::plugin::Auv3Plugin;

/// The main AUv3 wrapper struct.
///
/// This struct implements the AUv3 audio unit interface and translates calls
/// to the NIH-plug Plugin trait. AUv3 uses a modern architecture with app
/// extensions for sandboxed plugin hosting.
pub struct Auv3Wrapper<P: Plugin> {
    /// The wrapped plugin instance
    plugin: Mutex<P>,
    /// The plugin's parameters
    params: Arc<dyn Params>,
    /// Parameter pointers by index
    param_by_index: Vec<ParamPtr>,
    /// Parameter IDs by index for looking up parameter info
    param_id_by_index: Vec<String>,
    /// Current buffer configuration
    current_buffer_config: AtomicCell<Option<BufferConfig>>,
    /// Current audio IO layout
    current_audio_io_layout: AtomicCell<AudioIOLayout>,
    /// Incoming MIDI events for the current processing cycle
    input_events: AtomicRefCell<VecDeque<PluginNoteEvent<P>>>,
    /// Whether the plugin has been initialized
    is_initialized: AtomicCell<bool>,
    /// Whether the plugin is currently rendering
    is_rendering: AtomicCell<bool>,
}

impl<P: Plugin + Auv3Plugin> Auv3Wrapper<P> {
    /// Create a new AUv3 wrapper instance.
    pub fn new() -> Self {
        let plugin = P::default();
        let params = plugin.params();
        
        // Collect all parameters
        let param_by_index: Vec<ParamPtr> = params.param_map().into_iter().collect();
        let param_id_by_index: Vec<String> = param_by_index
            .iter()
            .map(|ptr| unsafe { ptr.id().to_string() })
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
            current_buffer_config: AtomicCell::new(None),
            current_audio_io_layout: AtomicCell::new(current_audio_io_layout),
            input_events: AtomicRefCell::new(VecDeque::new()),
            is_initialized: AtomicCell::new(false),
            is_rendering: AtomicCell::new(false),
        }
    }
    
    /// Allocate resources for rendering.
    ///
    /// This is called by the AUv3 host before rendering begins.
    /// It corresponds to the allocateRenderResourcesAndReturnError: method.
    pub fn allocate_render_resources(&mut self, sample_rate: f64, max_frames_per_slice: u32) -> bool {
        let config = BufferConfig {
            sample_rate: sample_rate as f32,
            min_buffer_size: None,
            max_buffer_size: max_frames_per_slice,
            process_mode: ProcessMode::Realtime,
        };
        
        self.current_buffer_config.store(Some(config));
        
        let mut plugin = self.plugin.lock();
        let layout = self.current_audio_io_layout.load();
        
        // Create a minimal init context
        // TODO: Implement proper InitContext for AUv3
        let success = plugin.initialize(
            &layout,
            &config,
            &mut crate::context::init::InitContext::default(),
        );
        
        if success {
            plugin.reset();
            self.is_initialized.store(true);
        }
        
        success
    }
    
    /// Deallocate rendering resources.
    ///
    /// This is called by the AUv3 host when rendering stops.
    /// It corresponds to the deallocateRenderResources method.
    pub fn deallocate_render_resources(&mut self) {
        let mut plugin = self.plugin.lock();
        plugin.deactivate();
        self.is_initialized.store(false);
        self.is_rendering.store(false);
    }
    
    /// Reset the plugin state.
    ///
    /// This is called by the AUv3 host to reset the plugin's internal state.
    pub fn reset(&mut self) {
        let mut plugin = self.plugin.lock();
        plugin.reset();
        self.input_events.borrow_mut().clear();
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
                param_ptr.set_normalized_value(value);
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
    ///
    /// AUv3 uses the same MIDI event format as AU, so we can reuse the translation logic.
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
    
    /// Render audio in real-time.
    ///
    /// This is the main audio processing function for AUv3 plugins.
    /// AUv3 uses a block-based rendering model similar to other modern plugin formats.
    /// This method is called from the audio thread and must be real-time safe.
    ///
    /// # Arguments
    ///
    /// * `input_buffers` - Input audio buffers (one per channel)
    /// * `output_buffers` - Output audio buffers (one per channel)
    /// * `num_frames` - Number of frames to process
    ///
    /// # Returns
    ///
    /// The processing status indicating whether the plugin produced output,
    /// has a tail, or encountered an error.
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
        
        self.is_rendering.store(true);
        
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
    ///
    /// This indicates how long the plugin continues to produce output
    /// after the input becomes silent (e.g., for reverb tails).
    pub fn get_tail_time(&self) -> Option<u32> {
        // TODO: Get actual tail time from plugin
        None
    }
    
    /// Check if the plugin supports real-time rendering.
    ///
    /// This corresponds to the shouldBypassEffect property in AUv3.
    pub fn should_bypass(&self) -> bool {
        // NIH-plug plugins are designed for real-time use
        false
    }
    
    /// Get the maximum number of frames the plugin can render.
    pub fn get_maximum_frames_per_slice(&self) -> u32 {
        self.current_buffer_config
            .load()
            .map(|config| config.max_buffer_size)
            .unwrap_or(4096)
    }
    
    /// Save the plugin state as a preset.
    ///
    /// Returns the serialized state data that can be stored by the AUv3 host.
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
    
    /// Get the full state for AUv3 state management.
    pub fn get_full_state(&self) -> Result<Vec<u8>, String> {
        self.save_preset()
    }
    
    /// Set the full state for AUv3 state management.
    pub fn set_full_state(&mut self, data: &[u8]) -> Result<(), String> {
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

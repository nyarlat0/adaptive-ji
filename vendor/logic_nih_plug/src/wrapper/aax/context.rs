//! AAX-specific context implementations.
//!
//! This module provides AAX-specific implementations of NIH-plug's context traits.

use std::sync::Arc;

use crate::context::init::InitContext;
use crate::context::process::{ProcessContext, Transport};
use crate::context::PluginApi;
use crate::prelude::{Plugin, Params};

/// AAX-specific initialization context.
pub struct AaxInitContext<P: Plugin> {
    _phantom: std::marker::PhantomData<P>,
}

impl<P: Plugin> AaxInitContext<P> {
    /// Create a new AAX initialization context.
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<P: Plugin> Default for AaxInitContext<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Plugin> InitContext<P> for AaxInitContext<P> {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Aax
    }

    fn execute(&self, _task: P::BackgroundTask) {
        // AAX doesn't have a built-in background task execution mechanism
        // This would need to be implemented by the host wrapper
    }

    fn set_latency_samples(&self, _samples: u32) {
        // AAX latency reporting would need to be implemented by the host wrapper
    }

    fn set_current_voice_capacity(&self, _capacity: u32) {
        // AAX voice capacity reporting would need to be implemented by the host wrapper
    }
}

/// AAX-specific process context.
///
/// This provides the context information needed during audio processing,
/// including transport state and parameter access.
pub struct AaxProcessContext<'a, P: Plugin> {
    /// Reference to the plugin's parameters
    pub params: &'a Arc<dyn Params>,
    /// Current transport state
    pub transport: Transport,
    /// Phantom data for the plugin type
    _phantom: std::marker::PhantomData<P>,
}

impl<'a, P: Plugin> AaxProcessContext<'a, P> {
    /// Create a new AAX process context.
    pub fn new(params: &'a Arc<dyn Params>, sample_rate: f32) -> Self {
        Self {
            params,
            transport: Transport::new(sample_rate),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Update the transport state from AAX host information.
    pub fn update_transport(
        &mut self,
        playing: bool,
        sample_rate: f64,
        tempo: Option<f64>,
        time_sig_numerator: Option<i32>,
        time_sig_denominator: Option<i32>,
        pos_samples: Option<i64>,
    ) {
        self.transport.playing = playing;
        self.transport.sample_rate = sample_rate as f32;
        self.transport.tempo = tempo;
        self.transport.time_sig_numerator = time_sig_numerator;
        self.transport.time_sig_denominator = time_sig_denominator;
        self.transport.pos_samples = pos_samples;

        // Calculate bar/beat position if we have tempo and time signature
        if let (Some(tempo), Some(pos)) = (tempo, pos_samples) {
            let samples_per_beat = (60.0 / tempo) * sample_rate;
            let beat_pos = pos as f64 / samples_per_beat;
            self.transport.pos_beats = Some(beat_pos);

            if let Some(numerator) = time_sig_numerator {
                let beats_per_bar = numerator as f64;
                self.transport.bar_number = Some((beat_pos / beats_per_bar).floor() as i32);
                self.transport.bar_start_pos_beats = Some(
                    (beat_pos / beats_per_bar).floor() * beats_per_bar,
                );
            }
        }
    }
}

impl<'a, P: Plugin> ProcessContext<P> for AaxProcessContext<'a, P> {
    fn plugin_api(&self) -> crate::context::PluginApi {
        crate::context::PluginApi::Aax
    }

    fn execute_background(&self, _task: P::BackgroundTask) {
        // AAX doesn't have a built-in background task execution mechanism
        // This would need to be implemented by the host wrapper
    }

    fn execute_gui(&self, _task: P::BackgroundTask) {
        // AAX doesn't have a built-in GUI task execution mechanism
        // This would need to be implemented by the host wrapper
    }

    fn next_event(&mut self) -> Option<crate::prelude::PluginNoteEvent<P>> {
        // Events are handled by the wrapper, not the context
        None
    }

    fn send_event(&mut self, _event: crate::prelude::PluginNoteEvent<P>) {
        // AAX doesn't support sending events from the plugin
        // This would need to be implemented by the host wrapper
    }

    fn set_latency_samples(&self, _samples: u32) {
        // AAX latency reporting would need to be implemented by the host wrapper
    }

    fn set_current_voice_capacity(&self, _capacity: u32) {
        // AAX voice capacity reporting would need to be implemented by the host wrapper
    }

    fn transport(&self) -> &Transport {
        &self.transport
    }
}

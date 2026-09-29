//! AU-specific context implementations.
//!
//! This module provides AU-specific implementations of NIH-plug's context traits.

use std::sync::Arc;
use crate::prelude::{
    Plugin, ProcessContext, Transport, Params, PluginNoteEvent, NoteEvent,
};

/// AU-specific process context.
///
/// This provides the context information and callbacks needed during
/// audio processing in an AU plugin.
pub struct AuProcessContext<'a, P: Plugin> {
    /// The plugin's parameters
    params: Arc<dyn Params>,
    /// Transport information
    transport: Transport,
    /// Input events for this processing cycle
    input_events: &'a [PluginNoteEvent<P>],
    /// Output events buffer
    output_events: Vec<PluginNoteEvent<P>>,
}

impl<'a, P: Plugin> AuProcessContext<'a, P> {
    /// Create a new AU process context.
    pub fn new(
        params: Arc<dyn Params>,
        transport: Transport,
        input_events: &'a [PluginNoteEvent<P>],
    ) -> Self {
        Self {
            params,
            transport,
            input_events,
            output_events: Vec::new(),
        }
    }
    
    /// Get the output events that were generated during processing.
    pub fn take_output_events(&mut self) -> Vec<PluginNoteEvent<P>> {
        std::mem::take(&mut self.output_events)
    }
}

impl<'a, P: Plugin> ProcessContext<P> for AuProcessContext<'a, P> {
    fn set_latency_samples(&self, _samples: u32) {
        // TODO: Implement latency reporting to AU host
    }

    fn set_current_voice_capacity(&self, _capacity: u32) {
        // Not applicable for AU
    }

    fn transport(&self) -> &Transport {
        &self.transport
    }

    fn next_event(&mut self) -> Option<PluginNoteEvent<P>> {
        // AU processes all events at once, not incrementally
        None
    }

    fn send_event(&mut self, event: PluginNoteEvent<P>) {
        self.output_events.push(event);
    }

    fn set_process_mode(&self, _mode: crate::prelude::ProcessMode) -> bool {
        // AU doesn't support changing process mode dynamically
        false
    }
}

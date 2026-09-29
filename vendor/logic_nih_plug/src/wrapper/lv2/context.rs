//! LV2-specific context implementations for InitContext and ProcessContext.

use std::sync::Arc;

use crate::context::init::InitContext;
use crate::context::process::ProcessContext;
use crate::plugin::Plugin;
use crate::prelude::{NoteEvent, PluginNoteEvent, Transport};

/// LV2 initialization context
pub struct Lv2InitContext<'a, P: Plugin> {
    _phantom: std::marker::PhantomData<&'a P>,
}

impl<'a, P: Plugin> Lv2InitContext<'a, P> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<P: Plugin> InitContext<P> for Lv2InitContext<'_, P> {
    fn plugin_api(&self) -> crate::prelude::PluginApi {
        crate::prelude::PluginApi::Lv2
    }

    fn execute(&self, task: P::BackgroundTask) {
        // LV2 doesn't have a built-in task execution mechanism
        // Tasks would need to be handled by the plugin itself
        log::debug!("Background task execution requested (not implemented for LV2)");
        drop(task);
    }

    fn set_latency_samples(&self, samples: u32) {
        // LV2 latency reporting would be handled through the latency port
        // For now, we'll store this and report it through the descriptor
        log::info!("Plugin reports latency: {} samples", samples);
    }

    fn set_current_voice_capacity(&self, capacity: u32) {
        // LV2 doesn't have a direct equivalent, but we can log it
        log::debug!("Voice capacity set to: {}", capacity);
    }
}

/// LV2 process context
pub struct Lv2ProcessContext<'a, P: Plugin> {
    pub transport: Transport,
    pub input_events: &'a [PluginNoteEvent<P>],
    pub output_events: &'a mut Vec<PluginNoteEvent<P>>,
}

impl<'a, P: Plugin> Lv2ProcessContext<'a, P> {
    pub fn new(
        transport: Transport,
        input_events: &'a [PluginNoteEvent<P>],
        output_events: &'a mut Vec<PluginNoteEvent<P>>,
    ) -> Self {
        Self {
            transport,
            input_events,
            output_events,
        }
    }
}

impl<P: Plugin> ProcessContext<P> for Lv2ProcessContext<'_, P> {
    fn plugin_api(&self) -> crate::prelude::PluginApi {
        crate::prelude::PluginApi::Lv2
    }

    fn execute_background(&self, task: P::BackgroundTask) {
        // LV2 doesn't have a built-in task execution mechanism
        log::debug!("Background task execution requested (not implemented for LV2)");
        drop(task);
    }

    fn execute_gui(&self, task: P::BackgroundTask) {
        // LV2 doesn't have a built-in GUI task execution mechanism
        log::debug!("GUI task execution requested (not implemented for LV2)");
        drop(task);
    }

    fn set_latency_samples(&self, samples: u32) {
        // LV2 latency changes during processing would need special handling
        log::debug!("Latency changed to: {} samples", samples);
    }

    fn set_current_voice_capacity(&self, capacity: u32) {
        log::debug!("Voice capacity changed to: {}", capacity);
    }

    fn transport(&self) -> &Transport {
        &self.transport
    }

    fn next_event(&mut self) -> Option<PluginNoteEvent<P>> {
        // Events are pre-sorted and provided in the input_events slice
        // This is a simplified implementation
        None
    }

    fn send_event(&mut self, event: PluginNoteEvent<P>) {
        self.output_events.push(event);
    }
}

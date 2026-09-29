//! AUv3-specific context implementations.
//!
//! This module provides AUv3-specific implementations of NIH-plug's context traits.

use std::sync::Arc;

use crate::prelude::{
    GuiContext, InitContext, ParamPtr, Params, Plugin, PluginNoteEvent, ProcessContext,
    Transport,
};

/// AUv3 implementation of InitContext.
///
/// This context is used during plugin initialization to provide access to
/// host information and allow the plugin to set up its initial state.
pub struct Auv3InitContext {
    // TODO: Add AUv3-specific context data
}

impl Auv3InitContext {
    pub fn new() -> Self {
        Self {}
    }
}

impl<P: Plugin> InitContext<P> for Auv3InitContext {
    fn set_latency_samples(&self, samples: u32) {
        // TODO: Notify AUv3 host of latency
    }

    fn set_current_voice_capacity(&self, capacity: u32) {
        // TODO: Notify AUv3 host of voice capacity
    }
}

/// AUv3 implementation of ProcessContext.
///
/// This context is used during audio processing to provide access to
/// transport information, parameter changes, and MIDI events.
pub struct Auv3ProcessContext<'a, P: Plugin> {
    pub(crate) transport: Transport,
    pub(crate) input_events: &'a [PluginNoteEvent<P>],
    pub(crate) output_events: &'a mut Vec<PluginNoteEvent<P>>,
}

impl<'a, P: Plugin> Auv3ProcessContext<'a, P> {
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

impl<'a, P: Plugin> ProcessContext<P> for Auv3ProcessContext<'a, P> {
    fn transport(&self) -> &Transport {
        &self.transport
    }

    fn next_event(&mut self) -> Option<PluginNoteEvent<P>> {
        // TODO: Implement proper event iteration
        None
    }

    fn send_event(&mut self, event: PluginNoteEvent<P>) {
        self.output_events.push(event);
    }

    fn set_latency_samples(&self, samples: u32) {
        // TODO: Notify AUv3 host of latency changes
    }

    fn set_current_voice_capacity(&self, capacity: u32) {
        // TODO: Notify AUv3 host of voice capacity changes
    }
}

/// AUv3 implementation of GuiContext.
///
/// This context is used by the plugin's GUI to interact with parameters
/// and request updates from the host.
pub struct Auv3GuiContext {
    params: Arc<dyn Params>,
}

impl Auv3GuiContext {
    pub fn new(params: Arc<dyn Params>) -> Self {
        Self { params }
    }
}

impl GuiContext for Auv3GuiContext {
    fn request_resize(&self) -> bool {
        // TODO: Implement resize request for AUv3
        false
    }

    fn get_state(&self) -> crate::prelude::PluginState {
        // TODO: Implement state retrieval
        crate::prelude::PluginState::default()
    }

    fn set_state(&self, state: crate::prelude::PluginState) {
        // TODO: Implement state setting
    }

    unsafe fn raw_begin_set_parameter(&self, param: ParamPtr) {
        // TODO: Notify AUv3 host that parameter editing has begun
    }

    unsafe fn raw_set_parameter_normalized(&self, param: ParamPtr, normalized: f32) {
        param.set_normalized_value(normalized);
        // TODO: Notify AUv3 host of parameter change
    }

    unsafe fn raw_end_set_parameter(&self, param: ParamPtr) {
        // TODO: Notify AUv3 host that parameter editing has ended
    }

    fn get_plugin_api(&self) -> crate::prelude::PluginApi {
        crate::prelude::PluginApi::Standalone
    }
}

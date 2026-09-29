//! Unified parameter automation handler for all plugin formats.
//!
//! This module provides a common interface for handling parameter automation
//! across different plugin formats (VST2, VST3, AU, AUv3, LV2, AAX, CLAP).
//!
//! The automation handler manages:
//! - Parameter value changes from the host
//! - Parameter change notifications to the host
//! - Smoother updates for parameter changes
//! - Format-specific notification mechanisms

use std::sync::Arc;
use atomic_refcell::AtomicRefCell;
use crossbeam::queue::ArrayQueue;

use crate::params::{internals::ParamPtr, Params};
use crate::prelude::BufferConfig;

/// Maximum number of parameter change events that can be queued
const PARAM_EVENT_QUEUE_CAPACITY: usize = 2048;

/// A parameter automation event that needs to be processed or sent to the host.
#[derive(Debug, Clone, Copy)]
pub enum ParamAutomationEvent {
    /// A parameter value has changed and needs to be sent to the host.
    /// Contains the parameter hash and the new normalized value.
    ValueChanged {
        param_hash: u32,
        normalized_value: f32,
    },
    /// Begin a parameter gesture (user started changing a parameter).
    BeginGesture { param_hash: u32 },
    /// End a parameter gesture (user finished changing a parameter).
    EndGesture { param_hash: u32 },
}

/// Unified parameter automation handler.
///
/// This struct provides a common interface for handling parameter automation
/// across all plugin formats. It manages parameter value changes, notifications,
/// and smoother updates.
pub struct ParamAutomationHandler {
    /// Queue of parameter events that need to be sent to the host
    outgoing_events: Arc<ArrayQueue<ParamAutomationEvent>>,
    /// The current sample rate, used for smoother updates
    current_sample_rate: AtomicRefCell<Option<f32>>,
}

impl ParamAutomationHandler {
    /// Create a new parameter automation handler.
    pub fn new() -> Self {
        Self {
            outgoing_events: Arc::new(ArrayQueue::new(PARAM_EVENT_QUEUE_CAPACITY)),
            current_sample_rate: AtomicRefCell::new(None),
        }
    }

    /// Update the current sample rate.
    ///
    /// This should be called when the buffer configuration changes.
    pub fn set_sample_rate(&self, sample_rate: f32) {
        *self.current_sample_rate.borrow_mut() = Some(sample_rate);
    }

    /// Set a parameter value from host automation.
    ///
    /// This updates the parameter's value and its smoother if a sample rate is available.
    /// Returns true if the value actually changed.
    ///
    /// # Arguments
    /// * `param_ptr` - Pointer to the parameter to update
    /// * `normalized_value` - The new normalized value (0.0 to 1.0)
    ///
    /// # Safety
    /// The caller must ensure that `param_ptr` is valid and points to a live parameter.
    pub unsafe fn set_parameter_from_host(
        &self,
        param_ptr: ParamPtr,
        normalized_value: f32,
    ) -> bool {
        // Set the parameter value
        let changed = param_ptr.set_normalized_value(normalized_value);

        if changed {
            // Update the smoother if we have a sample rate
            if let Some(sample_rate) = *self.current_sample_rate.borrow() {
                param_ptr.update_smoother(sample_rate, false);
            }
        }

        changed
    }

    /// Set a parameter value from the plugin (e.g., from the GUI).
    ///
    /// This updates the parameter's value, its smoother, and queues a notification
    /// event to be sent to the host.
    ///
    /// Returns true if the value actually changed and the event was queued successfully.
    ///
    /// # Arguments
    /// * `param_ptr` - Pointer to the parameter to update
    /// * `param_hash` - Hash of the parameter ID
    /// * `normalized_value` - The new normalized value (0.0 to 1.0)
    ///
    /// # Safety
    /// The caller must ensure that `param_ptr` is valid and points to a live parameter.
    pub unsafe fn set_parameter_from_plugin(
        &self,
        param_ptr: ParamPtr,
        param_hash: u32,
        normalized_value: f32,
    ) -> bool {
        // Set the parameter value
        let changed = param_ptr.set_normalized_value(normalized_value);

        if changed {
            // Update the smoother if we have a sample rate
            if let Some(sample_rate) = *self.current_sample_rate.borrow() {
                param_ptr.update_smoother(sample_rate, false);
            }

            // Queue a notification event for the host
            let event = ParamAutomationEvent::ValueChanged {
                param_hash,
                normalized_value,
            };
            let _ = self.outgoing_events.push(event);
        }

        changed
    }

    /// Begin a parameter gesture.
    ///
    /// This should be called when the user starts changing a parameter (e.g., mouse down on a slider).
    /// It queues a begin gesture event to be sent to the host.
    ///
    /// # Arguments
    /// * `param_hash` - Hash of the parameter ID
    pub fn begin_gesture(&self, param_hash: u32) {
        let event = ParamAutomationEvent::BeginGesture { param_hash };
        let _ = self.outgoing_events.push(event);
    }

    /// End a parameter gesture.
    ///
    /// This should be called when the user finishes changing a parameter (e.g., mouse up on a slider).
    /// It queues an end gesture event to be sent to the host.
    ///
    /// # Arguments
    /// * `param_hash` - Hash of the parameter ID
    pub fn end_gesture(&self, param_hash: u32) {
        let event = ParamAutomationEvent::EndGesture { param_hash };
        let _ = self.outgoing_events.push(event);
    }

    /// Get the next outgoing parameter event.
    ///
    /// This should be called by the wrapper to retrieve events that need to be
    /// sent to the host. Returns None if there are no pending events.
    pub fn pop_outgoing_event(&self) -> Option<ParamAutomationEvent> {
        self.outgoing_events.pop()
    }

    /// Check if there are any pending outgoing events.
    pub fn has_outgoing_events(&self) -> bool {
        !self.outgoing_events.is_empty()
    }

    /// Clear all pending outgoing events.
    ///
    /// This can be used when the plugin is being reset or deactivated.
    pub fn clear_outgoing_events(&self) {
        while self.outgoing_events.pop().is_some() {}
    }

    /// Update all parameter smoothers to the current values.
    ///
    /// This should be called when the plugin is initialized or when the buffer
    /// configuration changes. It resets all smoothers to their current parameter values.
    ///
    /// # Arguments
    /// * `params` - The plugin's parameters
    /// * `buffer_config` - The current buffer configuration
    pub fn update_all_smoothers(&self, params: &Arc<dyn Params>, buffer_config: &BufferConfig) {
        self.set_sample_rate(buffer_config.sample_rate);

        for (_id, param_ptr, _group) in params.param_map() {
            unsafe {
                param_ptr.update_smoother(buffer_config.sample_rate, true);
            }
        }
    }
}

impl Default for ParamAutomationHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_automation_handler_creation() {
        let handler = ParamAutomationHandler::new();
        assert!(!handler.has_outgoing_events());
    }

    #[test]
    fn test_sample_rate_update() {
        let handler = ParamAutomationHandler::new();
        handler.set_sample_rate(44100.0);
        assert_eq!(*handler.current_sample_rate.borrow(), Some(44100.0));
    }

    #[test]
    fn test_gesture_events() {
        let handler = ParamAutomationHandler::new();
        
        handler.begin_gesture(123);
        assert!(handler.has_outgoing_events());
        
        let event = handler.pop_outgoing_event();
        assert!(matches!(
            event,
            Some(ParamAutomationEvent::BeginGesture { param_hash: 123 })
        ));
        
        handler.end_gesture(123);
        let event = handler.pop_outgoing_event();
        assert!(matches!(
            event,
            Some(ParamAutomationEvent::EndGesture { param_hash: 123 })
        ));
        
        assert!(!handler.has_outgoing_events());
    }

    #[test]
    fn test_clear_events() {
        let handler = ParamAutomationHandler::new();
        
        handler.begin_gesture(1);
        handler.begin_gesture(2);
        handler.begin_gesture(3);
        
        assert!(handler.has_outgoing_events());
        
        handler.clear_outgoing_events();
        assert!(!handler.has_outgoing_events());
    }
}


/// Trait for format-specific parameter change notification.
///
/// Each plugin format wrapper should implement this trait to provide
/// format-specific notification mechanisms for parameter changes.
pub trait ParamChangeNotifier {
    /// Notify the host that a parameter value has changed.
    ///
    /// This is called when a parameter value changes from within the plugin
    /// (e.g., from the GUI or internal modulation).
    ///
    /// # Arguments
    /// * `param_hash` - Hash of the parameter ID
    /// * `normalized_value` - The new normalized value (0.0 to 1.0)
    fn notify_param_value_changed(&self, param_hash: u32, normalized_value: f32);

    /// Notify the host that a parameter gesture has begun.
    ///
    /// This is called when the user starts changing a parameter.
    ///
    /// # Arguments
    /// * `param_hash` - Hash of the parameter ID
    fn notify_begin_gesture(&self, param_hash: u32);

    /// Notify the host that a parameter gesture has ended.
    ///
    /// This is called when the user finishes changing a parameter.
    ///
    /// # Arguments
    /// * `param_hash` - Hash of the parameter ID
    fn notify_end_gesture(&self, param_hash: u32);

    /// Process all pending parameter automation events.
    ///
    /// This should be called by the wrapper at appropriate times (e.g., during
    /// audio processing or in a flush callback) to send queued parameter events
    /// to the host.
    ///
    /// # Arguments
    /// * `handler` - The parameter automation handler containing pending events
    fn process_param_events(&self, handler: &ParamAutomationHandler) {
        while let Some(event) = handler.pop_outgoing_event() {
            match event {
                ParamAutomationEvent::ValueChanged {
                    param_hash,
                    normalized_value,
                } => {
                    self.notify_param_value_changed(param_hash, normalized_value);
                }
                ParamAutomationEvent::BeginGesture { param_hash } => {
                    self.notify_begin_gesture(param_hash);
                }
                ParamAutomationEvent::EndGesture { param_hash } => {
                    self.notify_end_gesture(param_hash);
                }
            }
        }
    }
}

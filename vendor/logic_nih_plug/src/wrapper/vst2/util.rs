//! VST2 utility functions.
//!
//! This module contains helper functions for VST2 wrapper implementation.

/// Convert a normalized parameter value (0.0-1.0) to VST2 format.
pub fn normalized_to_vst2(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

/// Convert a VST2 parameter value to normalized format (0.0-1.0).
pub fn vst2_to_normalized(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

//! AUv3 utility functions.
//!
//! This module contains helper functions for working with AUv3 plugins.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// Convert a Rust string to a C string pointer.
///
/// The returned pointer must be freed by the caller using `free_c_string`.
pub fn to_c_string(s: &str) -> *mut c_char {
    match CString::new(s) {
        Ok(c_str) => c_str.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free a C string pointer created by `to_c_string`.
///
/// # Safety
///
/// The pointer must have been created by `to_c_string` and must not be used
/// after this function is called.
pub unsafe fn free_c_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        let _ = CString::from_raw(ptr);
    }
}

/// Convert a C string pointer to a Rust string.
///
/// Returns None if the pointer is null or the string is not valid UTF-8.
///
/// # Safety
///
/// The pointer must point to a valid null-terminated C string.
pub unsafe fn from_c_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    
    CStr::from_ptr(ptr)
        .to_str()
        .ok()
        .map(|s| s.to_string())
}

/// Convert a 4-character code to a u32 for AUv3 APIs.
pub fn fourcc_to_u32(fourcc: [u8; 4]) -> u32 {
    u32::from_be_bytes(fourcc)
}

/// Convert a u32 to a 4-character code for AUv3 APIs.
pub fn u32_to_fourcc(value: u32) -> [u8; 4] {
    value.to_be_bytes()
}

/// Convert a 4-character code to a string for debugging.
pub fn fourcc_to_string(fourcc: [u8; 4]) -> String {
    String::from_utf8_lossy(&fourcc).to_string()
}

/// Clamp a value to the range [0.0, 1.0].
pub fn clamp_normalized(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

/// Convert a MIDI note number to a frequency in Hz.
pub fn note_to_frequency(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}

/// Convert a frequency in Hz to a MIDI note number.
pub fn frequency_to_note(frequency: f32) -> u8 {
    (69.0 + 12.0 * (frequency / 440.0).log2()).round() as u8
}

/// Check if a plugin supports a specific audio IO configuration.
pub fn supports_audio_io(
    layouts: &[crate::prelude::AudioIOLayout],
    input_channels: u32,
    output_channels: u32,
) -> bool {
    layouts.iter().any(|layout| {
        let input_match = layout
            .main_input_channels
            .map(|ch| ch.get() == input_channels)
            .unwrap_or(input_channels == 0);
        
        let output_match = layout
            .main_output_channels
            .map(|ch| ch.get() == output_channels)
            .unwrap_or(output_channels == 0);
        
        input_match && output_match
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fourcc_conversion() {
        let fourcc = *b"aufx";
        let u32_val = fourcc_to_u32(fourcc);
        let back = u32_to_fourcc(u32_val);
        assert_eq!(fourcc, back);
    }

    #[test]
    fn test_clamp_normalized() {
        assert_eq!(clamp_normalized(-0.5), 0.0);
        assert_eq!(clamp_normalized(0.5), 0.5);
        assert_eq!(clamp_normalized(1.5), 1.0);
    }

    #[test]
    fn test_note_to_frequency() {
        // A4 = 440 Hz
        assert!((note_to_frequency(69) - 440.0).abs() < 0.01);
        // A5 = 880 Hz
        assert!((note_to_frequency(81) - 880.0).abs() < 0.01);
    }
}

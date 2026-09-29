//! AU utility functions.
//!
//! This module contains helper functions for AU wrapper implementation.

/// Convert a 4-character code to a u32.
pub fn four_char_code_to_u32(code: [u8; 4]) -> u32 {
    u32::from_be_bytes(code)
}

/// Convert a u32 to a 4-character code.
pub fn u32_to_four_char_code(value: u32) -> [u8; 4] {
    value.to_be_bytes()
}

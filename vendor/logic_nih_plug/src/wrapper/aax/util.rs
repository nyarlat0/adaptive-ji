//! AAX utility functions.
//!
//! This module contains helper functions for AAX wrapper implementation.

/// Convert a 4-character manufacturer ID to a u32.
pub fn manufacturer_id_to_u32(id: [u8; 4]) -> u32 {
    u32::from_be_bytes(id)
}

/// Convert a u32 to a 4-character manufacturer ID.
pub fn u32_to_manufacturer_id(value: u32) -> [u8; 4] {
    value.to_be_bytes()
}

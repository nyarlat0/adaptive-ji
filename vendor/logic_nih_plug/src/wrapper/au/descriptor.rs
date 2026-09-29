//! AU plugin descriptor and metadata handling.
//!
//! This module handles AU-specific plugin metadata and descriptor generation.

use crate::plugin::AuPlugin;
use crate::prelude::Plugin;

/// AU component descriptor.
///
/// This structure holds the metadata needed to register an AU component
/// with the system.
pub struct AuDescriptor {
    /// The AU type code (4 characters)
    pub au_type: [u8; 4],
    /// The AU subtype code (4 characters)
    pub subtype: [u8; 4],
    /// The AU manufacturer code (4 characters)
    pub manufacturer: [u8; 4],
    /// Plugin name
    pub name: String,
    /// Plugin version
    pub version: u32,
}

impl AuDescriptor {
    /// Create a new AU descriptor from a plugin.
    pub fn from_plugin<P: Plugin + AuPlugin>() -> Self {
        // Parse version string to integer
        let version = P::VERSION
            .split('.')
            .take(3)
            .enumerate()
            .fold(0u32, |acc, (i, part)| {
                let multiplier = match i {
                    0 => 10000,
                    1 => 100,
                    2 => 1,
                    _ => 0,
                };
                acc + part.parse::<u32>().unwrap_or(0) * multiplier
            });
        
        Self {
            au_type: P::AU_TYPE,
            subtype: P::AU_SUBTYPE,
            manufacturer: P::AU_MANUFACTURER,
            name: P::NAME.to_string(),
            version,
        }
    }
    
    /// Get the component type as a u32.
    pub fn component_type(&self) -> u32 {
        u32::from_be_bytes(self.au_type)
    }
    
    /// Get the component subtype as a u32.
    pub fn component_subtype(&self) -> u32 {
        u32::from_be_bytes(self.subtype)
    }
    
    /// Get the component manufacturer as a u32.
    pub fn component_manufacturer(&self) -> u32 {
        u32::from_be_bytes(self.manufacturer)
    }
}

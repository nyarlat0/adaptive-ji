//! AUv3 plugin descriptor and metadata handling.
//!
//! This module handles the conversion of NIH-plug plugin metadata to
//! AUv3-specific descriptor information.

use crate::plugin::Auv3Plugin;
use crate::prelude::Plugin;

/// AUv3 component descriptor.
///
/// This structure contains the metadata needed to register an AUv3 plugin
/// with the system. It corresponds to the AudioComponentDescription in the
/// AUv3 API.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Auv3ComponentDescription {
    /// The component type (e.g., 'aufx' for effects, 'aumu' for instruments)
    pub component_type: [u8; 4],
    /// The component subtype (unique identifier for this plugin)
    pub component_subtype: [u8; 4],
    /// The component manufacturer code
    pub component_manufacturer: [u8; 4],
    /// Component flags (reserved for future use)
    pub component_flags: u32,
    /// Component flags mask (reserved for future use)
    pub component_flags_mask: u32,
}

impl Auv3ComponentDescription {
    /// Create a new component description from a plugin type.
    pub fn from_plugin<P: Plugin + Auv3Plugin>() -> Self {
        Self {
            component_type: P::AUV3_COMPONENT_TYPE,
            component_subtype: P::AUV3_COMPONENT_SUBTYPE,
            component_manufacturer: P::AUV3_COMPONENT_MANUFACTURER,
            component_flags: 0,
            component_flags_mask: 0,
        }
    }
    
    /// Get the component type as a string for debugging.
    pub fn type_string(&self) -> String {
        String::from_utf8_lossy(&self.component_type).to_string()
    }
    
    /// Get the component subtype as a string for debugging.
    pub fn subtype_string(&self) -> String {
        String::from_utf8_lossy(&self.component_subtype).to_string()
    }
    
    /// Get the manufacturer code as a string for debugging.
    pub fn manufacturer_string(&self) -> String {
        String::from_utf8_lossy(&self.component_manufacturer).to_string()
    }
}

/// Get the plugin tags as an array of strings for AUv3 registration.
pub fn get_plugin_tags<P: Plugin + Auv3Plugin>() -> Vec<String> {
    P::AUV3_TAGS
        .iter()
        .map(|tag| tag.as_str().to_string())
        .collect()
}

/// Get the plugin name for AUv3 registration.
pub fn get_plugin_name<P: Plugin>() -> String {
    P::NAME.to_string()
}

/// Get the plugin manufacturer name for AUv3 registration.
pub fn get_plugin_manufacturer<P: Plugin>() -> String {
    P::VENDOR.to_string()
}

/// Get the plugin version for AUv3 registration.
pub fn get_plugin_version<P: Plugin>() -> String {
    P::VERSION.to_string()
}

/// Check if the plugin supports MIDI input.
pub fn supports_midi_input<P: Plugin>() -> bool {
    !matches!(P::MIDI_INPUT, crate::prelude::MidiConfig::None)
}

/// Check if the plugin supports MIDI output.
pub fn supports_midi_output<P: Plugin>() -> bool {
    !matches!(P::MIDI_OUTPUT, crate::prelude::MidiConfig::None)
}

/// Get the number of audio input channels.
pub fn get_input_channel_count<P: Plugin>() -> u32 {
    P::AUDIO_IO_LAYOUTS
        .first()
        .and_then(|layout| layout.main_input_channels)
        .map(|channels| channels.get())
        .unwrap_or(0)
}

/// Get the number of audio output channels.
pub fn get_output_channel_count<P: Plugin>() -> u32 {
    P::AUDIO_IO_LAYOUTS
        .first()
        .and_then(|layout| layout.main_output_channels)
        .map(|channels| channels.get())
        .unwrap_or(0)
}

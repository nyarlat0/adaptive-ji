//! Audio Units-specific plugin trait and metadata.

use crate::prelude::Plugin;

/// AU plugin type categories. These correspond to the standard AU type codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuType {
    /// Effect plugin (`aufx`)
    Effect,
    /// Music effect plugin (`aumf`)
    MusicEffect,
    /// Instrument/synthesizer plugin (`aumu`)
    Instrument,
    /// Generator plugin (`augn`)
    Generator,
    /// MIDI processor plugin (`aumi`)
    MidiProcessor,
    /// Offline effect plugin (`auol`)
    OfflineEffect,
    /// Panner plugin (`aupn`)
    Panner,
    /// Format converter plugin (`aufc`)
    FormatConverter,
    /// Output plugin (`auou`)
    Output,
}

impl AuType {
    /// Returns the 4-character AU type code for this type.
    pub fn as_type_code(&self) -> [u8; 4] {
        match self {
            AuType::Effect => *b"aufx",
            AuType::MusicEffect => *b"aumf",
            AuType::Instrument => *b"aumu",
            AuType::Generator => *b"augn",
            AuType::MidiProcessor => *b"aumi",
            AuType::OfflineEffect => *b"auol",
            AuType::Panner => *b"aupn",
            AuType::FormatConverter => *b"aufc",
            AuType::Output => *b"auou",
        }
    }
}

/// AU-specific plugin metadata trait.
///
/// Audio Units is Apple's plugin format for macOS. Plugins that want to be exported
/// as AU plugins must implement this trait in addition to the main `Plugin` trait.
///
/// # Example
///
/// ```ignore
/// use logic_nih_plug::prelude::*;
///
/// struct MyPlugin {
///     params: Arc<MyParams>,
/// }
///
/// impl Plugin for MyPlugin {
///     // ... standard Plugin implementation
/// }
///
/// #[cfg(target_os = "macos")]
/// impl AuPlugin for MyPlugin {
///     const AU_TYPE: [u8; 4] = *b"aufx";  // Effect plugin
///     const AU_SUBTYPE: [u8; 4] = *b"MyPl";
///     const AU_MANUFACTURER: [u8; 4] = *b"Mfgr";
/// }
///
/// // Export the plugin as AU
/// #[cfg(target_os = "macos")]
/// nih_export_au!(MyPlugin);
/// ```
///
/// # Platform Support
///
/// - **macOS only** - AU is not available on other platforms
/// - Generates `.component` bundle
/// - Requires Component Manager registration
///
/// # Four-Character Codes
///
/// All AU identifiers use 4-character codes (FourCC). These should be:
/// - Unique to your plugin/company
/// - Consistent across plugin versions
/// - Never changed after release
///
/// # Testing
///
/// Use Apple's `auval` tool to validate your AU plugin:
/// ```bash
/// auval -v aufx MyPl Mfgr
/// ```
///
/// See the [Multi-Format Export Guide](../../../MULTI_FORMAT_EXPORT.md#au-export)
/// for detailed information.
#[cfg(target_os = "macos")]
pub trait AuPlugin: Plugin {
    /// The AU type code (4 characters).
    ///
    /// This identifies the general category of your plugin.
    ///
    /// # Common Values
    ///
    /// - `*b"aufx"` - Effect plugin
    /// - `*b"aumu"` - Instrument/synthesizer plugin
    /// - `*b"aumf"` - Music effect plugin
    /// - `*b"aumi"` - MIDI processor plugin
    ///
    /// See [`AuType`] for all available types.
    const AU_TYPE: [u8; 4];

    /// The AU subtype code (4 characters).
    ///
    /// This should be a unique identifier for your specific plugin.
    /// It's recommended to use the same subtype as your VST2 plugin if you have one.
    ///
    /// # Important
    ///
    /// This code must be unique and should **never change** after release.
    ///
    /// # Example
    ///
    /// ```ignore
    /// const AU_SUBTYPE: [u8; 4] = *b"MyPl";
    /// ```
    const AU_SUBTYPE: [u8; 4];

    /// The AU manufacturer code (4 characters).
    ///
    /// This should be a unique identifier for you or your company.
    /// Use the same manufacturer code across all your plugins for consistency.
    ///
    /// # Example
    ///
    /// ```ignore
    /// const AU_MANUFACTURER: [u8; 4] = *b"Mfgr";
    /// ```
    const AU_MANUFACTURER: [u8; 4];
}

//! Audio Units v3 (AUv3)-specific plugin trait and metadata.

use crate::prelude::Plugin;

/// AUv3 plugin tags. These are used by hosts to categorize and filter plugins.
/// Tags help users discover plugins in the AUv3 browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auv3Tag {
    /// Effect plugin
    Effect,
    /// Synthesizer/instrument plugin
    Synth,
    /// Delay effect
    Delay,
    /// Distortion effect
    Distortion,
    /// Dynamics processing (compressor, limiter, etc.)
    Dynamics,
    /// EQ effect
    EQ,
    /// Filter effect
    Filter,
    /// Reverb effect
    Reverb,
    /// Modulation effect (chorus, flanger, phaser, etc.)
    Modulation,
    /// Pitch shifting effect
    PitchShift,
    /// Spatial/panning effect
    Spatial,
    /// Generator (oscillator, noise, etc.)
    Generator,
    /// MIDI effect/processor
    MIDIEffect,
    /// Mixer plugin
    Mixer,
    /// Sampler plugin
    Sampler,
    /// Utility plugin
    Utility,
}

impl Auv3Tag {
    /// Returns the string representation of this tag for AUv3.
    pub fn as_str(&self) -> &'static str {
        match self {
            Auv3Tag::Effect => "Effect",
            Auv3Tag::Synth => "Synth",
            Auv3Tag::Delay => "Delay",
            Auv3Tag::Distortion => "Distortion",
            Auv3Tag::Dynamics => "Dynamics",
            Auv3Tag::EQ => "EQ",
            Auv3Tag::Filter => "Filter",
            Auv3Tag::Reverb => "Reverb",
            Auv3Tag::Modulation => "Modulation",
            Auv3Tag::PitchShift => "Pitch Shift",
            Auv3Tag::Spatial => "Spatial",
            Auv3Tag::Generator => "Generator",
            Auv3Tag::MIDIEffect => "MIDI Effect",
            Auv3Tag::Mixer => "Mixer",
            Auv3Tag::Sampler => "Sampler",
            Auv3Tag::Utility => "Utility",
        }
    }
}

/// AUv3-specific plugin metadata trait.
///
/// AUv3 (Audio Units version 3) is Apple's modern plugin format for iOS and macOS,
/// using app extensions for sandboxed plugin hosting. Plugins that want to be exported
/// as AUv3 plugins must implement this trait in addition to the main `Plugin` trait.
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
/// #[cfg(all(feature = "auv3", target_os = "macos"))]
/// impl Auv3Plugin for MyPlugin {
///     const AUV3_COMPONENT_TYPE: [u8; 4] = *b"aufx";  // Effect
///     const AUV3_COMPONENT_SUBTYPE: [u8; 4] = *b"MyPl";
///     const AUV3_COMPONENT_MANUFACTURER: [u8; 4] = *b"Mfgr";
///     const AUV3_TAGS: &'static [Auv3Tag] = &[Auv3Tag::Effect, Auv3Tag::Delay];
/// }
///
/// // Export the plugin as AUv3
/// #[cfg(all(feature = "auv3", target_os = "macos"))]
/// nih_export_auv3!(MyPlugin);
/// ```
///
/// # Platform Support
///
/// - macOS 10.11+
/// - iOS 9.0+
///
/// # App Extension Requirements
///
/// AUv3 plugins require additional setup beyond the Rust code:
///
/// 1. **Xcode Project**: Create an app extension target
/// 2. **Info.plist**: Configure NSExtension and AudioComponents
/// 3. **Code Signing**: Sign with valid developer certificate
/// 4. **Entitlements**: Set up appropriate entitlements
///
/// # Key Differences from AU
///
/// - Uses app extension architecture (sandboxed)
/// - Requires Xcode project setup
/// - Modern iOS/macOS integration
/// - View controller-based UI
/// - Inter-process communication
///
/// # Testing
///
/// - **iOS**: Test in AUM or other AUv3 hosts
/// - **macOS**: Test in Logic Pro or GarageBand
///
/// See the [Multi-Format Export Guide](../../../MULTI_FORMAT_EXPORT.md#auv3-export)
/// for detailed information including Info.plist configuration.
#[cfg(all(feature = "auv3", target_os = "macos"))]
pub trait Auv3Plugin: Plugin {
    /// The AUv3 component type code (4 characters).
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
    /// # Example
    ///
    /// ```ignore
    /// const AUV3_COMPONENT_TYPE: [u8; 4] = *b"aufx";
    /// ```
    const AUV3_COMPONENT_TYPE: [u8; 4];

    /// The AUv3 component subtype code (4 characters).
    ///
    /// This should be a unique identifier for your plugin.
    /// It's recommended to use the same subtype as your AU plugin if you have one.
    ///
    /// # Important
    ///
    /// This code must be unique and should **never change** after release.
    ///
    /// # Example
    ///
    /// ```ignore
    /// const AUV3_COMPONENT_SUBTYPE: [u8; 4] = *b"MyPl";
    /// ```
    const AUV3_COMPONENT_SUBTYPE: [u8; 4];

    /// The AUv3 component manufacturer code (4 characters).
    ///
    /// This should be a unique identifier for you or your company.
    /// It's recommended to use the same manufacturer code across all your plugins.
    ///
    /// # Example
    ///
    /// ```ignore
    /// const AUV3_COMPONENT_MANUFACTURER: [u8; 4] = *b"Mfgr";
    /// ```
    const AUV3_COMPONENT_MANUFACTURER: [u8; 4];

    /// Tags for categorizing the plugin in the AUv3 browser.
    ///
    /// These help users discover your plugin in the AUv3 browser.
    /// You can specify multiple tags to improve discoverability.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Delay effect plugin
    /// const AUV3_TAGS: &'static [Auv3Tag] = &[Auv3Tag::Effect, Auv3Tag::Delay];
    ///
    /// // Synthesizer
    /// const AUV3_TAGS: &'static [Auv3Tag] = &[Auv3Tag::Synth, Auv3Tag::Generator];
    /// ```
    ///
    /// See [`Auv3Tag`] for all available tags.
    const AUV3_TAGS: &'static [Auv3Tag];
}

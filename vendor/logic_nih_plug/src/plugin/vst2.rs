use super::Plugin;

/// VST2 plugin categories. These are used by hosts to organize plugins in their plugin browsers.
/// See the VST2 SDK documentation for more information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vst2Category {
    /// Unknown or unspecified category
    Unknown,
    /// Effect plugin
    Effect,
    /// Synthesizer plugin
    Synth,
    /// Analysis plugin
    Analysis,
    /// Mastering plugin
    Mastering,
    /// Spacializer plugin
    Spacializer,
    /// Room effect plugin
    RoomFx,
    /// Surround effect plugin
    SurroundFx,
    /// Restoration plugin
    Restoration,
    /// Offline processing plugin
    OfflineProcess,
    /// Shell plugin (container for multiple plugins)
    Shell,
    /// Generator plugin
    Generator,
}

impl Vst2Category {
    /// Returns the VST2 category constant value
    pub fn as_vst2_constant(&self) -> i32 {
        match self {
            Vst2Category::Unknown => 0,
            Vst2Category::Effect => 1,
            Vst2Category::Synth => 2,
            Vst2Category::Analysis => 3,
            Vst2Category::Mastering => 4,
            Vst2Category::Spacializer => 5,
            Vst2Category::RoomFx => 6,
            Vst2Category::SurroundFx => 7,
            Vst2Category::Restoration => 8,
            Vst2Category::OfflineProcess => 9,
            Vst2Category::Shell => 10,
            Vst2Category::Generator => 11,
        }
    }
}

/// Provides auxiliary metadata needed for a VST2 plugin.
///
/// VST2 is Steinberg's legacy Virtual Studio Technology format. While officially deprecated,
/// it remains widely used in many DAWs and production environments.
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
/// impl Vst2Plugin for MyPlugin {
///     const VST2_UNIQUE_ID: i32 = i32::from_be_bytes(*b"MyPl");
///     const VST2_CATEGORY: Vst2Category = Vst2Category::Effect;
/// }
///
/// // Export the plugin as VST2
/// nih_export_vst2!(MyPlugin);
/// ```
///
/// # Platform Support
///
/// - Windows: Generates `.dll` files
/// - macOS: Generates `.vst` bundle
/// - Linux: Generates `.so` files
///
/// # Important Notes
///
/// - VST2 is deprecated by Steinberg and no new licenses are being issued
/// - The VST2_UNIQUE_ID must never change after release
/// - VST2 has limited parameter automation compared to VST3
/// - Maximum 16 MIDI channels supported
///
/// See the [Multi-Format Export Guide](../../../MULTI_FORMAT_EXPORT.md#vst2-export)
/// for detailed information.
pub trait Vst2Plugin: Plugin {
    /// The unique plugin ID. This is a 32-bit integer that should be unique to your plugin.
    /// You can generate this using a four-character code, e.g., `i32::from_be_bytes(*b"MyPl")`.
    /// 
    /// # Important
    /// 
    /// This ID is used by hosts to identify your plugin and should **never change** once released,
    /// as changing it will break existing projects that use your plugin.
    ///
    /// # Example
    ///
    /// ```ignore
    /// const VST2_UNIQUE_ID: i32 = i32::from_be_bytes(*b"MyPl");
    /// ```
    const VST2_UNIQUE_ID: i32;
    
    /// The plugin category. This helps hosts organize plugins in their plugin browsers.
    ///
    /// Choose the category that best describes your plugin's primary function.
    /// See [`Vst2Category`] for available options.
    const VST2_CATEGORY: Vst2Category;
}

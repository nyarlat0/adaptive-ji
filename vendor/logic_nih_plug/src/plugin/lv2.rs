//! LV2-specific plugin trait and metadata.

use crate::prelude::Plugin;

/// LV2 plugin categories. These correspond to standard LV2 plugin classes.
/// See http://lv2plug.in/ns/lv2core for more information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lv2Category {
    /// Plugin category
    Plugin,
    /// Delay effect
    DelayPlugin,
    /// Distortion effect
    DistortionPlugin,
    /// Dynamics processor (compressor, limiter, gate, etc.)
    DynamicsPlugin,
    /// Equalizer
    EQPlugin,
    /// Filter
    FilterPlugin,
    /// Modulation effect (chorus, flanger, phaser, etc.)
    ModulatorPlugin,
    /// Reverb effect
    ReverbPlugin,
    /// Simulator (amp sim, etc.)
    SimulatorPlugin,
    /// Spatial/panning effect
    SpatialPlugin,
    /// Spectral processor
    SpectralPlugin,
    /// Utility plugin
    UtilityPlugin,
    /// Analyzer
    AnalyserPlugin,
    /// Converter
    ConverterPlugin,
    /// Function generator
    FunctionPlugin,
    /// Mixer
    MixerPlugin,
    /// Instrument/synthesizer
    InstrumentPlugin,
    /// Oscillator
    OscillatorPlugin,
    /// Generator
    GeneratorPlugin,
    /// MIDI utility
    MIDIPlugin,
}

impl Lv2Category {
    /// Returns the LV2 class URI for this category.
    pub fn as_uri(&self) -> &'static str {
        match self {
            Lv2Category::Plugin => "http://lv2plug.in/ns/lv2core#Plugin",
            Lv2Category::DelayPlugin => "http://lv2plug.in/ns/lv2core#DelayPlugin",
            Lv2Category::DistortionPlugin => "http://lv2plug.in/ns/lv2core#DistortionPlugin",
            Lv2Category::DynamicsPlugin => "http://lv2plug.in/ns/lv2core#DynamicsPlugin",
            Lv2Category::EQPlugin => "http://lv2plug.in/ns/lv2core#EQPlugin",
            Lv2Category::FilterPlugin => "http://lv2plug.in/ns/lv2core#FilterPlugin",
            Lv2Category::ModulatorPlugin => "http://lv2plug.in/ns/lv2core#ModulatorPlugin",
            Lv2Category::ReverbPlugin => "http://lv2plug.in/ns/lv2core#ReverbPlugin",
            Lv2Category::SimulatorPlugin => "http://lv2plug.in/ns/lv2core#SimulatorPlugin",
            Lv2Category::SpatialPlugin => "http://lv2plug.in/ns/lv2core#SpatialPlugin",
            Lv2Category::SpectralPlugin => "http://lv2plug.in/ns/lv2core#SpectralPlugin",
            Lv2Category::UtilityPlugin => "http://lv2plug.in/ns/lv2core#UtilityPlugin",
            Lv2Category::AnalyserPlugin => "http://lv2plug.in/ns/lv2core#AnalyserPlugin",
            Lv2Category::ConverterPlugin => "http://lv2plug.in/ns/lv2core#ConverterPlugin",
            Lv2Category::FunctionPlugin => "http://lv2plug.in/ns/lv2core#FunctionPlugin",
            Lv2Category::MixerPlugin => "http://lv2plug.in/ns/lv2core#MixerPlugin",
            Lv2Category::InstrumentPlugin => "http://lv2plug.in/ns/lv2core#InstrumentPlugin",
            Lv2Category::OscillatorPlugin => "http://lv2plug.in/ns/lv2core#OscillatorPlugin",
            Lv2Category::GeneratorPlugin => "http://lv2plug.in/ns/lv2core#GeneratorPlugin",
            Lv2Category::MIDIPlugin => "http://lv2plug.in/ns/lv2core#MIDIPlugin",
        }
    }
}

/// LV2-specific plugin metadata trait.
///
/// LV2 is an open-source plugin standard primarily used on Linux, though it's
/// cross-platform. Plugins that want to be exported as LV2 plugins must implement
/// this trait in addition to the main `Plugin` trait.
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
/// impl Lv2Plugin for MyPlugin {
///     const LV2_URI: &'static str = "http://example.org/plugins/my-plugin";
///     const LV2_CATEGORY: Lv2Category = Lv2Category::DelayPlugin;
/// }
///
/// // Export the plugin as LV2
/// nih_export_lv2!(MyPlugin);
/// ```
///
/// # Platform Support
///
/// - Linux: Primary platform, full support
/// - macOS: Supported
/// - Windows: Supported but less common
///
/// # Bundle Structure
///
/// LV2 plugins are distributed as bundles:
/// ```text
/// my-plugin.lv2/
/// ├── manifest.ttl      # Bundle manifest (auto-generated)
/// ├── my-plugin.ttl     # Plugin description (auto-generated)
/// └── my-plugin.so      # Plugin binary
/// ```
///
/// The bundler automatically generates the `.ttl` files based on your plugin's metadata.
///
/// # Testing
///
/// Use `lv2lint` to validate your LV2 plugin:
/// ```bash
/// lv2lint http://example.org/plugins/my-plugin
/// ```
///
/// Test in LV2 hosts like Ardour, Qtractor, or Carla.
///
/// See the [Multi-Format Export Guide](../../../MULTI_FORMAT_EXPORT.md#lv2-export)
/// for detailed information.
pub trait Lv2Plugin: Plugin {
    /// The LV2 URI for this plugin.
    ///
    /// This must be a valid URI that uniquely identifies your plugin.
    /// It's recommended to use a URI under a domain you control.
    ///
    /// # Format
    ///
    /// The URI should follow this pattern:
    /// - `http://example.org/plugins/my-plugin`
    /// - `https://yourcompany.com/lv2/plugin-name`
    ///
    /// # Important
    ///
    /// This URI is used by hosts to identify your plugin and should **never change**
    /// once released, as changing it will break existing projects.
    ///
    /// # Example
    ///
    /// ```ignore
    /// const LV2_URI: &'static str = "http://example.org/plugins/my-plugin";
    /// ```
    const LV2_URI: &'static str;

    /// The LV2 category for this plugin.
    ///
    /// This helps hosts organize plugins in their plugin browsers.
    /// Choose the category that best describes your plugin's primary function.
    ///
    /// See [`Lv2Category`] for available options.
    ///
    /// # Example
    ///
    /// ```ignore
    /// const LV2_CATEGORY: Lv2Category = Lv2Category::DelayPlugin;
    /// ```
    const LV2_CATEGORY: Lv2Category;
}

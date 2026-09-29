//! AAX-specific plugin trait and metadata.

use crate::prelude::Plugin;
use crate::wrapper::aax::descriptor::{AaxCategory, AaxTypeId};

/// AAX-specific plugin metadata trait.
///
/// AAX (Avid Audio eXtension) is Avid's plugin format for Pro Tools. Plugins that
/// want to be exported as AAX plugins must implement this trait in addition to the
/// main `Plugin` trait.
///
/// # Important Requirements
///
/// AAX requires the proprietary AAX SDK from Avid, which requires:
/// - Avid developer account
/// - Signed developer agreement
/// - Annual developer program fee
/// - Code signing certificate from Avid
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
/// impl AaxPlugin for MyPlugin {
///     const AAX_MANUFACTURER_ID: [u8; 4] = *b"Mfgr";
///     const AAX_PRODUCT_ID: i32 = 0x12345678;
///     const AAX_CATEGORY: AaxCategory = AaxCategory::Effect;
///     const AAX_TYPE_IDS: &'static [AaxTypeId] = &[AaxTypeId::Native];
/// }
///
/// // Export the plugin as AAX
/// nih_export_aax!(MyPlugin);
/// ```
///
/// # Platform Support
///
/// - Windows: Supported
/// - macOS: Supported
/// - Linux: Not supported
///
/// # Current Implementation Status
///
/// The current AAX implementation provides the structure and trait definitions but
/// requires full AAX SDK integration to function as a real AAX plugin. This is due
/// to the proprietary nature of the AAX SDK.
///
/// # Getting Started with AAX
///
/// 1. Register for Avid developer program
/// 2. Download AAX SDK
/// 3. Obtain manufacturer ID and product IDs
/// 4. Get code signing certificate
/// 5. Integrate AAX SDK with your build system
///
/// See the [Multi-Format Export Guide](../../../MULTI_FORMAT_EXPORT.md#aax-export)
/// for detailed information.
pub trait AaxPlugin: Plugin {
    /// The AAX manufacturer ID (4 characters).
    ///
    /// This must be obtained from Avid as part of the developer program.
    /// The manufacturer ID is assigned by Avid and identifies you or your company.
    ///
    /// # Important
    ///
    /// - Must be obtained from Avid (cannot be self-assigned)
    /// - Use the same manufacturer ID across all your AAX plugins
    /// - Required for code signing
    ///
    /// # Example
    ///
    /// ```ignore
    /// const AAX_MANUFACTURER_ID: [u8; 4] = *b"Mfgr";
    /// ```
    const AAX_MANUFACTURER_ID: [u8; 4];

    /// The AAX product ID.
    ///
    /// This should be a unique identifier for your plugin. Product IDs are
    /// assigned by Avid as part of the developer program.
    ///
    /// # Example
    ///
    /// ```ignore
    /// const AAX_PRODUCT_ID: i32 = 0x12345678;
    /// ```
    const AAX_PRODUCT_ID: i32;

    /// The AAX category for this plugin.
    ///
    /// This helps Pro Tools organize plugins in the plugin browser.
    /// See [`AaxCategory`] for available options.
    const AAX_CATEGORY: AaxCategory;

    /// The AAX type IDs (Native, AudioSuite, etc.).
    ///
    /// Most plugins will use `&[AaxTypeId::Native]` for real-time processing.
    /// AudioSuite is for offline processing.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Real-time plugin
    /// const AAX_TYPE_IDS: &'static [AaxTypeId] = &[AaxTypeId::Native];
    ///
    /// // Both real-time and offline
    /// const AAX_TYPE_IDS: &'static [AaxTypeId] = &[
    ///     AaxTypeId::Native,
    ///     AaxTypeId::AudioSuite,
    /// ];
    /// ```
    const AAX_TYPE_IDS: &'static [AaxTypeId];
}

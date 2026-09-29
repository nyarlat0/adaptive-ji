//! LV2 plugin wrapper module.
//!
//! This module provides the LV2 wrapper implementation for NIH-plug plugins.
//! LV2 is an open-source plugin standard primarily used on Linux.

#[cfg(feature = "lv2")]
pub mod wrapper;
#[cfg(feature = "lv2")]
pub mod context;
#[cfg(feature = "lv2")]
pub mod descriptor;
#[cfg(feature = "lv2")]
pub mod util;
#[cfg(feature = "lv2")]
pub mod atom;
#[cfg(feature = "lv2")]
pub mod state;
#[cfg(feature = "lv2")]
pub mod manifest;

// Re-export the main wrapper type
#[cfg(feature = "lv2")]
pub use wrapper::{Lv2Wrapper, Lv2Descriptor, Lv2Handle, Lv2Feature};

/// Export macro for LV2 plugins. This generates the necessary LV2 entry points.
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
///     // ... Plugin implementation
/// }
///
/// impl Lv2Plugin for MyPlugin {
///     const LV2_URI: &'static str = "http://example.org/plugins/my-plugin";
///     const LV2_CATEGORY: Lv2Category = Lv2Category::DelayPlugin;
/// }
///
/// nih_export_lv2!(MyPlugin);
/// ```
#[cfg(feature = "lv2")]
#[macro_export]
macro_rules! nih_export_lv2 {
    ($plugin_ty:ty) => {
        /// LV2 descriptor function
        /// This is the main entry point that LV2 hosts call to get the plugin descriptor
        #[no_mangle]
        pub extern "C" fn lv2_descriptor(index: u32) -> *const $crate::wrapper::lv2::Lv2Descriptor {
            $crate::wrapper::setup_logger();

            // LV2 hosts call this function with increasing indices until we return null
            // Since we only have one plugin per library, we return the descriptor for index 0
            if index == 0 {
                $crate::wrapper::lv2::Lv2Wrapper::<$plugin_ty>::get_descriptor()
            } else {
                ::std::ptr::null()
            }
        }

        /// Generate manifest.ttl at compile time
        /// This would typically be done by a build script or bundler
        #[cfg(feature = "lv2")]
        pub fn generate_lv2_manifest() -> String {
            use $crate::plugin::Lv2Plugin;
            $crate::wrapper::lv2::descriptor::generate_manifest_ttl::<$plugin_ty>()
        }

        /// Generate plugin.ttl at compile time
        /// This would typically be done by a build script or bundler
        #[cfg(feature = "lv2")]
        pub fn generate_lv2_plugin_ttl() -> String {
            use $crate::plugin::Lv2Plugin;
            let port_descriptors = $crate::wrapper::lv2::descriptor::generate_port_descriptors::<$plugin_ty>();
            $crate::wrapper::lv2::descriptor::generate_plugin_ttl::<$plugin_ty>(&port_descriptors)
        }

        /// Generate and validate the complete LV2 bundle
        /// Returns (manifest.ttl, plugin.ttl) or an error message
        #[cfg(feature = "lv2")]
        pub fn generate_lv2_bundle() -> Result<(String, String), String> {
            use $crate::plugin::Lv2Plugin;
            $crate::wrapper::lv2::manifest::generate_and_validate_bundle::<$plugin_ty>()
        }
    };
}

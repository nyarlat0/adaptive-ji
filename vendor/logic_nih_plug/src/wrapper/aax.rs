//! AAX (Avid Audio eXtension) plugin wrapper module.
//!
//! This module provides the AAX wrapper implementation for NIH-plug plugins.
//! AAX is Avid's plugin format for Pro Tools.
//!
//! Note: AAX requires the proprietary AAX SDK from Avid, which requires
//! a developer account and agreement with Avid.

#[cfg(feature = "aax")]
pub mod context;
#[cfg(feature = "aax")]
pub mod descriptor;
#[cfg(feature = "aax")]
pub mod util;
#[cfg(feature = "aax")]
pub mod wrapper;

// Re-export the main wrapper type
#[cfg(feature = "aax")]
pub use wrapper::AaxWrapper;

/// Export macro for AAX plugins. This generates the necessary AAX entry points.
///
/// Note: This is a placeholder macro. Full AAX support requires integration with
/// the proprietary AAX SDK from Avid. The AAX SDK provides the actual entry point
/// definitions and plugin registration mechanisms.
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
/// impl AaxPlugin for MyPlugin {
///     const AAX_MANUFACTURER_ID: [u8; 4] = *b"Mfgr";
///     const AAX_PRODUCT_ID: i32 = 0x12345678;
///     const AAX_CATEGORY: AaxCategory = AaxCategory::Effect;
///     const AAX_TYPE_IDS: &'static [AaxTypeId] = &[AaxTypeId::Native];
/// }
///
/// nih_export_aax!(MyPlugin);
/// ```
///
/// # AAX SDK Integration
///
/// To use this macro with the actual AAX SDK, you would need to:
/// 1. Obtain the AAX SDK from Avid (requires developer account)
/// 2. Link against the AAX SDK libraries
/// 3. Implement the AAX_Effect interface using the wrapper
/// 4. Register the plugin with Pro Tools using AAX SDK functions
///
/// The current implementation provides the structure but requires AAX SDK
/// integration to function as a real AAX plugin.
#[cfg(feature = "aax")]
#[macro_export]
macro_rules! nih_export_aax {
    ($plugin_ty:ty) => {
        // AAX plugin descriptor function
        // This would be called by the AAX SDK to get plugin information
        #[no_mangle]
        pub extern "C" fn AAX_GetPlugInDescriptor() -> *const ::std::os::raw::c_void {
            $crate::wrapper::setup_logger();

            // In a real AAX implementation, this would return an AAX plugin descriptor
            // that the AAX SDK can use to instantiate the plugin.
            // For now, we return a null pointer as a placeholder.
            //
            // With the AAX SDK, you would:
            // 1. Create an AAX_IEffectDescriptor
            // 2. Set up the plugin properties (name, ID, category, etc.)
            // 3. Register parameter descriptors
            // 4. Register audio processing callbacks
            // 5. Return the descriptor pointer
            ::std::ptr::null()
        }

        // AAX plugin creation function
        // This would be called by the AAX SDK to create a plugin instance
        #[no_mangle]
        pub extern "C" fn AAX_CreateEffectInstance() -> *mut ::std::os::raw::c_void {
            // In a real AAX implementation, this would:
            // 1. Create a new AaxWrapper<$plugin_ty> instance
            // 2. Wrap it in an AAX_IEffectParameters implementation
            // 3. Return the pointer to the AAX SDK
            //
            // For now, we create a wrapper but return null since we don't have
            // the AAX SDK to properly integrate with.
            let _wrapper = Box::new($crate::wrapper::aax::AaxWrapper::<$plugin_ty>::new());
            ::std::ptr::null_mut()
        }

        // Platform-specific entry points
        #[cfg(target_os = "windows")]
        #[no_mangle]
        pub extern "system" fn DllMain(
            _hinst_dll: *mut ::std::os::raw::c_void,
            _fdw_reason: u32,
            _lpv_reserved: *mut ::std::os::raw::c_void,
        ) -> i32 {
            1 // TRUE
        }

        #[cfg(target_os = "macos")]
        #[no_mangle]
        pub extern "C" fn AAX_BundleMain(
            _bundle: *mut ::std::os::raw::c_void,
        ) -> *mut ::std::os::raw::c_void {
            ::std::ptr::null_mut()
        }
    };
}

// Re-export the macro when the feature is enabled
#[cfg(feature = "aax")]
pub use nih_export_aax;

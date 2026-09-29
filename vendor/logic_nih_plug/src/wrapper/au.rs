//! Audio Units (AU) plugin wrapper module.
//!
//! This module provides the AU wrapper implementation for NIH-plug plugins.
//! AU is Apple's plugin format for macOS and iOS.

#[cfg(all(feature = "au", target_os = "macos"))]
pub mod wrapper;
#[cfg(all(feature = "au", target_os = "macos"))]
pub mod context;
#[cfg(all(feature = "au", target_os = "macos"))]
pub mod descriptor;
#[cfg(all(feature = "au", target_os = "macos"))]
pub mod util;

// Re-export the main wrapper type
#[cfg(all(feature = "au", target_os = "macos"))]
pub use wrapper::AuWrapper;

/// Export macro for Audio Units plugins. This generates the necessary AU component entry points.
///
/// Audio Units use the Component Manager on macOS to register and instantiate plugins.
/// This macro generates the required entry points and component registration code.
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
/// impl AuPlugin for MyPlugin {
///     const AU_TYPE: [u8; 4] = *b"aufx";  // Effect plugin
///     const AU_SUBTYPE: [u8; 4] = *b"MyPl";
///     const AU_MANUFACTURER: [u8; 4] = *b"Mfgr";
/// }
///
/// nih_export_au!(MyPlugin);
/// ```
#[cfg(all(feature = "au", target_os = "macos"))]
#[macro_export]
macro_rules! nih_export_au {
    ($plugin_ty:ty) => {
        // AU component factory function
        // This is the main entry point that the Component Manager calls to create instances
        #[no_mangle]
        pub extern "C" fn AudioComponentFactoryFunction(
            desc: *const $crate::wrapper::au::AuComponentDescription,
        ) -> *mut ::std::ffi::c_void {
            $crate::wrapper::setup_logger();
            
            if desc.is_null() {
                return ::std::ptr::null_mut();
            }
            
            // Create a new wrapper instance
            let wrapper = Box::new($crate::wrapper::au::AuWrapper::<$plugin_ty>::new());
            Box::into_raw(wrapper) as *mut ::std::ffi::c_void
        }
        
        // Component registration information
        // This structure is used by the Component Manager to identify the plugin
        #[no_mangle]
        #[used]
        pub static AU_COMPONENT_DESCRIPTION: $crate::wrapper::au::AuComponentDescription = {
            use $crate::plugin::AuPlugin;
            $crate::wrapper::au::AuComponentDescription {
                component_type: <$plugin_ty as AuPlugin>::AU_TYPE,
                component_subtype: <$plugin_ty as AuPlugin>::AU_SUBTYPE,
                component_manufacturer: <$plugin_ty as AuPlugin>::AU_MANUFACTURER,
                component_flags: 0,
                component_flags_mask: 0,
            }
        };
    };
}

/// AU Component Description structure.
///
/// This structure describes an AU component for registration with the Component Manager.
#[cfg(all(feature = "au", target_os = "macos"))]
#[repr(C)]
pub struct AuComponentDescription {
    /// The component type (e.g., 'aufx' for effects)
    pub component_type: [u8; 4],
    /// The component subtype (unique identifier for this plugin)
    pub component_subtype: [u8; 4],
    /// The component manufacturer code
    pub component_manufacturer: [u8; 4],
    /// Component flags
    pub component_flags: u32,
    /// Component flags mask
    pub component_flags_mask: u32,
}

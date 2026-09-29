//! Audio Units v3 (AUv3) plugin wrapper module.
//!
//! This module provides the AUv3 wrapper implementation for NIH-plug plugins.
//! AUv3 is Apple's modern plugin format for iOS and macOS, using app extensions
//! for sandboxed plugin hosting.
//!
//! # Platform Support
//!
//! AUv3 is only available on macOS and iOS. This module is only compiled when
//! both the `auv3` feature is enabled and the target OS is macOS.
//!
//! # App Extension Architecture
//!
//! AUv3 plugins are distributed as app extensions, which requires additional
//! Xcode project setup beyond just the Rust code. The plugin must be packaged
//! as an app extension bundle with the appropriate Info.plist and entitlements.

#[cfg(all(feature = "auv3", target_os = "macos"))]
pub mod wrapper;
#[cfg(all(feature = "auv3", target_os = "macos"))]
pub mod context;
#[cfg(all(feature = "auv3", target_os = "macos"))]
pub mod descriptor;
#[cfg(all(feature = "auv3", target_os = "macos"))]
pub mod util;

// Re-export the main wrapper type
#[cfg(all(feature = "auv3", target_os = "macos"))]
pub use wrapper::Auv3Wrapper;
#[cfg(all(feature = "auv3", target_os = "macos"))]
pub use descriptor::Auv3ComponentDescription;

/// Export macro for Audio Units v3 plugins.
///
/// This macro generates the necessary AUv3 app extension entry points and
/// component registration code. AUv3 plugins require additional Xcode project
/// setup to create the app extension bundle.
///
/// # Platform Requirements
///
/// - macOS 10.11+ or iOS 9.0+
/// - Xcode project with app extension target
/// - Proper Info.plist configuration
/// - Code signing with appropriate entitlements
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
/// impl Auv3Plugin for MyPlugin {
///     const AUV3_COMPONENT_TYPE: [u8; 4] = *b"aufx";  // Effect plugin
///     const AUV3_COMPONENT_SUBTYPE: [u8; 4] = *b"MyPl";
///     const AUV3_COMPONENT_MANUFACTURER: [u8; 4] = *b"Mfgr";
///     const AUV3_TAGS: &'static [Auv3Tag] = &[Auv3Tag::Effect, Auv3Tag::Delay];
/// }
///
/// nih_export_auv3!(MyPlugin);
/// ```
///
/// # App Extension Setup
///
/// After implementing the plugin, you need to:
///
/// 1. Create an app extension target in Xcode
/// 2. Configure the Info.plist with:
///    - NSExtension dictionary
///    - NSExtensionPrincipalClass pointing to your audio unit class
///    - AudioComponents array with component description
/// 3. Link against the AudioToolbox framework
/// 4. Set up code signing with appropriate entitlements
/// 5. Build the app extension bundle
///
/// See Apple's AUv3 documentation for detailed setup instructions.
#[cfg(all(feature = "auv3", target_os = "macos"))]
#[macro_export]
macro_rules! nih_export_auv3 {
    ($plugin_ty:ty) => {
        // AUv3 uses a different architecture than AU/VST2
        // The plugin is instantiated through the AudioComponentInstantiate API
        // which is typically called from Objective-C/Swift code in the app extension
        
        /// Create a new AUv3 wrapper instance.
        ///
        /// This function is called by the AUv3 host (via Objective-C/Swift glue code)
        /// to create a new instance of the plugin.
        #[no_mangle]
        pub extern "C" fn create_auv3_instance() -> *mut ::std::ffi::c_void {
            $crate::wrapper::setup_logger();
            
            let wrapper = Box::new($crate::wrapper::auv3::Auv3Wrapper::<$plugin_ty>::new());
            Box::into_raw(wrapper) as *mut ::std::ffi::c_void
        }
        
        /// Destroy an AUv3 wrapper instance.
        ///
        /// This function is called by the AUv3 host to clean up the plugin instance.
        #[no_mangle]
        pub extern "C" fn destroy_auv3_instance(instance: *mut ::std::ffi::c_void) {
            if !instance.is_null() {
                unsafe {
                    let _ = Box::from_raw(instance as *mut $crate::wrapper::auv3::Auv3Wrapper<$plugin_ty>);
                }
            }
        }
        
        /// Get the component description for this plugin.
        ///
        /// This is used by the AUv3 host to identify and register the plugin.
        #[no_mangle]
        pub extern "C" fn get_auv3_component_description() -> $crate::wrapper::auv3::Auv3ComponentDescription {
            use $crate::plugin::Auv3Plugin;
            $crate::wrapper::auv3::Auv3ComponentDescription {
                component_type: <$plugin_ty as Auv3Plugin>::AUV3_COMPONENT_TYPE,
                component_subtype: <$plugin_ty as Auv3Plugin>::AUV3_COMPONENT_SUBTYPE,
                component_manufacturer: <$plugin_ty as Auv3Plugin>::AUV3_COMPONENT_MANUFACTURER,
                component_flags: 0,
                component_flags_mask: 0,
            }
        }
        
        /// Get the plugin tags as a C string array.
        ///
        /// Returns a null-terminated array of C strings representing the plugin tags.
        /// The caller is responsible for freeing the returned array and strings.
        #[no_mangle]
        pub extern "C" fn get_auv3_tags(count: *mut usize) -> *mut *mut ::std::os::raw::c_char {
            use $crate::plugin::Auv3Plugin;
            
            if count.is_null() {
                return ::std::ptr::null_mut();
            }
            
            let tags = <$plugin_ty as Auv3Plugin>::AUV3_TAGS;
            unsafe {
                *count = tags.len();
            }
            
            if tags.is_empty() {
                return ::std::ptr::null_mut();
            }
            
            // Allocate array of string pointers
            let mut tag_ptrs: Vec<*mut ::std::os::raw::c_char> = tags
                .iter()
                .map(|tag| $crate::wrapper::auv3::util::to_c_string(tag.as_str()))
                .collect();
            
            let ptr = tag_ptrs.as_mut_ptr();
            ::std::mem::forget(tag_ptrs);
            ptr
        }
        
        /// Free the tag array returned by get_auv3_tags.
        #[no_mangle]
        pub extern "C" fn free_auv3_tags(tags: *mut *mut ::std::os::raw::c_char, count: usize) {
            if tags.is_null() {
                return;
            }
            
            unsafe {
                for i in 0..count {
                    let tag_ptr = *tags.add(i);
                    $crate::wrapper::auv3::util::free_c_string(tag_ptr);
                }
                
                // Free the array itself
                let _ = Vec::from_raw_parts(tags, count, count);
            }
        }
    };
}

//! VST2 plugin wrapper implementation.
//!
//! This module provides the VST2 wrapper that translates between the VST2 API and NIH-plug's
//! Plugin trait. The wrapper handles parameter management, audio processing, MIDI event
//! translation, and editor integration.

pub mod context;
pub mod descriptor;
pub mod util;
pub mod wrapper;

/// Re-export the main wrapper type
pub use wrapper::Vst2Wrapper;

/// Export macro for VST2 plugins. This generates the necessary VST2 entry points.
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
/// impl Vst2Plugin for MyPlugin {
///     const VST2_UNIQUE_ID: i32 = i32::from_be_bytes(*b"MyPl");
///     const VST2_CATEGORY: Vst2Category = Vst2Category::Effect;
/// }
///
/// nih_export_vst2!(MyPlugin);
/// ```
#[macro_export]
macro_rules! nih_export_vst2 {
    ($plugin_ty:ty) => {
        // VST2 entry point for the plugin (primary entry point)
        #[no_mangle]
        pub extern "C" fn VSTPluginMain(
            host_callback: $crate::wrapper::vst2::wrapper::HostCallbackProc,
        ) -> *mut $crate::wrapper::vst2::wrapper::AEffect {
            $crate::wrapper::setup_logger();
            $crate::wrapper::vst2::wrapper::Vst2Wrapper::<$plugin_ty>::new(host_callback)
        }

        // Alternative entry point name used by some hosts (macOS)
        #[cfg(target_os = "macos")]
        #[no_mangle]
        pub extern "C" fn main_macho(
            host_callback: $crate::wrapper::vst2::wrapper::HostCallbackProc,
        ) -> *mut $crate::wrapper::vst2::wrapper::AEffect {
            VSTPluginMain(host_callback)
        }

        // Alternative entry point name used by some hosts
        #[no_mangle]
        pub extern "C" fn main(
            host_callback: $crate::wrapper::vst2::wrapper::HostCallbackProc,
        ) -> *mut $crate::wrapper::vst2::wrapper::AEffect {
            VSTPluginMain(host_callback)
        }

        // Windows-specific entry point (system calling convention)
        #[cfg(target_os = "windows")]
        #[no_mangle]
        pub extern "system" fn MAIN(
            host_callback: $crate::wrapper::vst2::wrapper::HostCallbackProc,
        ) -> *mut $crate::wrapper::vst2::wrapper::AEffect {
            VSTPluginMain(host_callback)
        }

        // Additional Windows entry point
        #[cfg(target_os = "windows")]
        #[no_mangle]
        pub extern "C" fn main_plugin(
            host_callback: $crate::wrapper::vst2::wrapper::HostCallbackProc,
        ) -> *mut $crate::wrapper::vst2::wrapper::AEffect {
            VSTPluginMain(host_callback)
        }
    };
}

//! Wrappers for different plugin types. Each wrapper has an entry point macro that you can pass the
//! name of a type that implements `Plugin` to. The macro will handle the rest.

pub mod clap;
pub mod param_automation;
pub mod state;
pub(crate) mod util;

#[cfg(test)]
mod audio_buffer_routing_tests;

#[cfg(feature = "standalone")]
pub mod standalone;
#[cfg(feature = "vst3")]
pub mod vst3;
#[cfg(feature = "vst2")]
pub mod vst2;
#[cfg(all(feature = "au", target_os = "macos"))]
pub mod au;
#[cfg(all(feature = "auv3", target_os = "macos"))]
pub mod auv3;
#[cfg(feature = "lv2")]
pub mod lv2;
#[cfg(feature = "aax")]
pub mod aax;

// This is used by the wrappers.
pub use util::setup_logger;

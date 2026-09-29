//! Different contexts the plugin can use to make callbacks to the host in different...contexts.

use std::fmt::Display;

pub mod gui;
pub mod init;
pub mod process;

// Contexts for more plugin-API specific features
pub mod remote_controls;

/// The currently active plugin API. This may be useful to display in an about screen in the
/// plugin's GUI for debugging purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginApi {
    Clap,
    Standalone,
    Vst3,
    #[cfg(feature = "vst2")]
    Vst2,
    #[cfg(all(feature = "au", target_os = "macos"))]
    Au,
    #[cfg(all(feature = "auv3", target_os = "macos"))]
    Auv3,
    #[cfg(feature = "lv2")]
    Lv2,
    #[cfg(feature = "aax")]
    Aax,
}

impl Display for PluginApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginApi::Clap => write!(f, "CLAP"),
            PluginApi::Standalone => write!(f, "standalone"),
            PluginApi::Vst3 => write!(f, "VST3"),
            #[cfg(feature = "vst2")]
            PluginApi::Vst2 => write!(f, "VST2"),
            #[cfg(all(feature = "au", target_os = "macos"))]
            PluginApi::Au => write!(f, "AU"),
            #[cfg(all(feature = "auv3", target_os = "macos"))]
            PluginApi::Auv3 => write!(f, "AUv3"),
            #[cfg(feature = "lv2")]
            PluginApi::Lv2 => write!(f, "LV2"),
            #[cfg(feature = "aax")]
            PluginApi::Aax => write!(f, "AAX"),
        }
    }
}

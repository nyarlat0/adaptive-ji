// Used in [`AudioIOLayout`]
pub use std::num::NonZeroU32;

// Re-export the macros, derive macros are already re-exported from their respective modules
pub use crate::debug::*;

pub use crate::nih_export_clap;
#[cfg(feature = "vst3")]
pub use crate::nih_export_vst3;
#[cfg(feature = "vst2")]
pub use crate::nih_export_vst2;
#[cfg(all(feature = "au", target_os = "macos"))]
pub use crate::nih_export_au;
#[cfg(all(feature = "auv3", target_os = "macos"))]
pub use crate::nih_export_auv3;
#[cfg(feature = "lv2")]
pub use crate::nih_export_lv2;
#[cfg(feature = "aax")]
pub use crate::nih_export_aax;
#[cfg(feature = "standalone")]
pub use crate::wrapper::standalone::{nih_export_standalone, nih_export_standalone_with_args};

pub use crate::formatters;
pub use crate::util;

pub use crate::audio_setup::{
    new_nonzero_u32, AudioIOLayout, AuxiliaryBuffers, BufferConfig, PortNames, ProcessMode,
};
pub use crate::buffer::Buffer;
pub use crate::context::gui::{AsyncExecutor, GuiContext, ParamSetter};
pub use crate::context::init::InitContext;
pub use crate::context::process::{ProcessContext, Transport};
pub use crate::context::remote_controls::{
    RemoteControlsContext, RemoteControlsPage, RemoteControlsSection,
};
pub use crate::context::PluginApi;
// This also includes the derive macro
pub use crate::editor::{Editor, ParentWindowHandle};
pub use crate::midi::sysex::SysExMessage;
pub use crate::midi::{control_change, MidiConfig, NoteEvent, PluginNoteEvent};
pub use crate::params::enums::{Enum, EnumParam};
pub use crate::params::internals::ParamPtr;
pub use crate::params::range::{FloatRange, IntRange};
pub use crate::params::smoothing::{AtomicF32, Smoothable, Smoother, SmoothingStyle};
pub use crate::params::Params;
pub use crate::params::{BoolParam, FloatParam, IntParam, Param, ParamFlags};
pub use crate::plugin::clap::{ClapPlugin, PolyModulationConfig};
#[cfg(feature = "vst3")]
pub use crate::plugin::vst3::Vst3Plugin;
#[cfg(feature = "vst2")]
pub use crate::plugin::vst2::Vst2Plugin;
#[cfg(all(feature = "au", target_os = "macos"))]
pub use crate::plugin::au::AuPlugin;
#[cfg(all(feature = "auv3", target_os = "macos"))]
pub use crate::plugin::auv3::Auv3Plugin;
#[cfg(feature = "lv2")]
pub use crate::plugin::lv2::Lv2Plugin;
#[cfg(feature = "aax")]
pub use crate::plugin::aax::AaxPlugin;
pub use crate::plugin::{Plugin, ProcessStatus, TaskExecutor};
pub use crate::wrapper::clap::features::ClapFeature;
pub use crate::wrapper::state::PluginState;
#[cfg(feature = "vst3")]
pub use crate::wrapper::vst3::subcategories::Vst3SubCategory;
#[cfg(feature = "vst2")]
pub use crate::plugin::vst2::Vst2Category;
#[cfg(all(feature = "au", target_os = "macos"))]
pub use crate::wrapper::au::descriptor::AuType;
#[cfg(all(feature = "auv3", target_os = "macos"))]
pub use crate::wrapper::auv3::descriptor::Auv3Tag;
#[cfg(feature = "lv2")]
pub use crate::plugin::lv2::Lv2Category;
#[cfg(feature = "aax")]
pub use crate::wrapper::aax::descriptor::{AaxCategory, AaxTypeId};

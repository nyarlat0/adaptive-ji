//! VST2-specific context implementations.
//!
//! This module provides VST2-specific implementations of NIH-plug's context traits.

use crate::prelude::Plugin;

/// VST2-specific process context.
pub struct Vst2ProcessContext<P: Plugin> {
    _phantom: std::marker::PhantomData<P>,
}

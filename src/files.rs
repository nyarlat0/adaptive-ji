//! Standalone-only filesystem boundary. The portable core parses text.
use adaptive_ji_core::tuning::{
    profile::SpectrumProfile,
    scala::{KeyboardMapping, ScalaScale},
};
use std::path::Path;
fn parse_file<T>(
    path: &Path,
    kind: &str,
    parse: impl FnOnce(&str) -> Result<T, String>,
) -> Result<T, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {kind} {}: {e}", path.display()))?;
    parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}
pub fn load_scale(path: &Path) -> Result<ScalaScale, String> {
    parse_file(path, "scale", ScalaScale::parse)
}
pub fn load_mapping(path: &Path) -> Result<KeyboardMapping, String> {
    parse_file(path, "mapping", KeyboardMapping::parse)
}
pub fn load_spectrum(path: &Path) -> Result<SpectrumProfile, String> {
    parse_file(path, "spectrum file", SpectrumProfile::from_toml_str)
}

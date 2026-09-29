//! LV2 manifest generation and validation.

use crate::plugin::lv2::Lv2Plugin;
use crate::plugin::Plugin;

use super::descriptor::{generate_manifest_ttl, generate_plugin_ttl, generate_port_descriptors};

/// Generate both manifest.ttl and plugin.ttl files for an LV2 plugin
pub fn generate_lv2_bundle<P: Plugin + Lv2Plugin>() -> (String, String) {
    let manifest = generate_manifest_ttl::<P>();
    let port_descriptors = generate_port_descriptors::<P>();
    let plugin_ttl = generate_plugin_ttl::<P>(&port_descriptors);

    (manifest, plugin_ttl)
}

/// Validate RDF/Turtle syntax (basic validation)
pub fn validate_turtle_syntax(ttl: &str) -> Result<(), String> {
    // Basic validation checks
    let lines: Vec<&str> = ttl.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Check for basic Turtle syntax elements
        if trimmed.starts_with('@') {
            // Directive line (e.g., @prefix)
            if !trimmed.ends_with('.') && !trimmed.ends_with(';') {
                return Err(format!(
                    "Line {}: Directive should end with '.' or ';'",
                    i + 1
                ));
            }
        } else if trimmed.ends_with('.') || trimmed.ends_with(';') || trimmed.ends_with(',') {
            // Valid statement ending
            continue;
        } else if trimmed.ends_with('[') || trimmed.ends_with(']') {
            // Blank node syntax
            continue;
        } else if i + 1 < lines.len() {
            // Check if the next line continues the statement
            let next_trimmed = lines[i + 1].trim();
            if next_trimmed.starts_with(';')
                || next_trimmed.starts_with(',')
                || next_trimmed.starts_with('.')
                || next_trimmed.starts_with(']')
            {
                continue;
            }
        }
    }

    // Check for balanced brackets
    let open_brackets = ttl.matches('[').count();
    let close_brackets = ttl.matches(']').count();
    if open_brackets != close_brackets {
        return Err(format!(
            "Unbalanced brackets: {} open, {} close",
            open_brackets, close_brackets
        ));
    }

    Ok(())
}

/// Validate that the manifest contains required elements
pub fn validate_manifest<P: Plugin + Lv2Plugin>(manifest: &str) -> Result<(), String> {
    // Check for required prefixes
    if !manifest.contains("@prefix lv2:") {
        return Err("Missing @prefix lv2: declaration".to_string());
    }

    if !manifest.contains("@prefix rdfs:") {
        return Err("Missing @prefix rdfs: declaration".to_string());
    }

    // Check for plugin URI
    if !manifest.contains(P::LV2_URI) {
        return Err(format!("Missing plugin URI: {}", P::LV2_URI));
    }

    // Check for required properties
    if !manifest.contains("a lv2:Plugin") {
        return Err("Missing 'a lv2:Plugin' declaration".to_string());
    }

    if !manifest.contains("lv2:binary") {
        return Err("Missing lv2:binary property".to_string());
    }

    if !manifest.contains("rdfs:seeAlso") {
        return Err("Missing rdfs:seeAlso property".to_string());
    }

    // Validate Turtle syntax
    validate_turtle_syntax(manifest)?;

    Ok(())
}

/// Validate that the plugin.ttl contains required elements
pub fn validate_plugin_ttl<P: Plugin + Lv2Plugin>(plugin_ttl: &str) -> Result<(), String> {
    // Check for required prefixes
    if !plugin_ttl.contains("@prefix lv2:") {
        return Err("Missing @prefix lv2: declaration".to_string());
    }

    // Check for plugin URI
    if !plugin_ttl.contains(P::LV2_URI) {
        return Err(format!("Missing plugin URI: {}", P::LV2_URI));
    }

    // Check for plugin name
    if !plugin_ttl.contains("doap:name") {
        return Err("Missing doap:name property".to_string());
    }

    // Check for plugin category
    if !plugin_ttl.contains(P::LV2_CATEGORY.as_uri()) {
        return Err(format!(
            "Missing plugin category: {}",
            P::LV2_CATEGORY.as_uri()
        ));
    }

    // Check for ports
    if !plugin_ttl.contains("lv2:port") {
        return Err("Missing lv2:port declarations".to_string());
    }

    // Validate Turtle syntax
    validate_turtle_syntax(plugin_ttl)?;

    Ok(())
}

/// Generate and validate the complete LV2 bundle
pub fn generate_and_validate_bundle<P: Plugin + Lv2Plugin>() -> Result<(String, String), String> {
    let (manifest, plugin_ttl) = generate_lv2_bundle::<P>();

    // Validate both files
    validate_manifest::<P>(&manifest)?;
    validate_plugin_ttl::<P>(&plugin_ttl)?;

    Ok((manifest, plugin_ttl))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_turtle_syntax_valid() {
        let valid_ttl = r#"
@prefix lv2: <http://lv2plug.in/ns/lv2core#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

<http://example.org/plugin>
    a lv2:Plugin ;
    rdfs:label "Test Plugin" .
"#;
        assert!(validate_turtle_syntax(valid_ttl).is_ok());
    }

    #[test]
    fn test_validate_turtle_syntax_unbalanced_brackets() {
        let invalid_ttl = r#"
<http://example.org/plugin>
    a lv2:Plugin ;
    lv2:port [
        a lv2:AudioPort .
"#;
        assert!(validate_turtle_syntax(invalid_ttl).is_err());
    }
}

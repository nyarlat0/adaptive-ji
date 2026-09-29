//! LV2 descriptor and plugin metadata handling.

use std::ffi::CString;
use std::os::raw::c_char;
use std::ptr;

use crate::plugin::lv2::Lv2Plugin;
use crate::plugin::Plugin;

/// LV2 port types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lv2PortType {
    /// Audio input port
    AudioInput,
    /// Audio output port
    AudioOutput,
    /// Control input port (parameter)
    ControlInput,
    /// Control output port
    ControlOutput,
    /// Atom input port (for MIDI/events)
    AtomInput,
    /// Atom output port (for MIDI/events)
    AtomOutput,
}

/// LV2 port descriptor
pub struct Lv2PortDescriptor {
    pub index: u32,
    pub port_type: Lv2PortType,
    pub name: String,
    pub symbol: String,
}

/// Generate LV2 port descriptors for a plugin
pub fn generate_port_descriptors<P: Plugin + Lv2Plugin>() -> Vec<Lv2PortDescriptor> {
    let mut ports = Vec::new();
    let mut port_index = 0;

    // Get audio IO layout
    let audio_io_layout = P::AUDIO_IO_LAYOUTS
        .first()
        .expect("Plugin must have at least one audio IO layout");

    // Add audio input ports
    if let Some(num_inputs) = audio_io_layout.main_input_channels {
        for i in 0..num_inputs.get() {
            ports.push(Lv2PortDescriptor {
                index: port_index,
                port_type: Lv2PortType::AudioInput,
                name: format!("Audio Input {}", i + 1),
                symbol: format!("audio_in_{}", i + 1),
            });
            port_index += 1;
        }
    }

    // Add audio output ports
    if let Some(num_outputs) = audio_io_layout.main_output_channels {
        for i in 0..num_outputs.get() {
            ports.push(Lv2PortDescriptor {
                index: port_index,
                port_type: Lv2PortType::AudioOutput,
                name: format!("Audio Output {}", i + 1),
                symbol: format!("audio_out_{}", i + 1),
            });
            port_index += 1;
        }
    }

    // Add MIDI input port if needed
    if P::MIDI_INPUT != crate::prelude::MidiConfig::None {
        ports.push(Lv2PortDescriptor {
            index: port_index,
            port_type: Lv2PortType::AtomInput,
            name: "MIDI Input".to_string(),
            symbol: "midi_in".to_string(),
        });
        port_index += 1;
    }

    // Add MIDI output port if needed
    if P::MIDI_OUTPUT != crate::prelude::MidiConfig::None {
        ports.push(Lv2PortDescriptor {
            index: port_index,
            port_type: Lv2PortType::AtomOutput,
            name: "MIDI Output".to_string(),
            symbol: "midi_out".to_string(),
        });
        port_index += 1;
    }

    // Add control ports for parameters
    // Note: This will be populated when we have access to the plugin instance
    // For now, we'll add them dynamically in the wrapper

    ports
}

/// Generate the manifest.ttl content for an LV2 plugin
pub fn generate_manifest_ttl<P: Plugin + Lv2Plugin>() -> String {
    format!(
        r#"@prefix lv2:  <http://lv2plug.in/ns/lv2core#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

<{uri}>
    a lv2:Plugin ;
    lv2:binary <{binary_name}.so> ;
    rdfs:seeAlso <{uri}.ttl> .
"#,
        uri = P::LV2_URI,
        binary_name = P::NAME.to_lowercase().replace(' ', "_")
    )
}

/// Generate the plugin.ttl content for an LV2 plugin
pub fn generate_plugin_ttl<P: Plugin + Lv2Plugin>(port_descriptors: &[Lv2PortDescriptor]) -> String {
    let mut ttl = format!(
        r#"@prefix lv2:   <http://lv2plug.in/ns/lv2core#> .
@prefix rdfs:  <http://www.w3.org/2000/01/rdf-schema#> .
@prefix doap:  <http://usefulinc.com/ns/doap#> .
@prefix foaf:  <http://xmlns.com/foaf/0.1/> .
@prefix atom:  <http://lv2plug.in/ns/ext/atom#> .
@prefix urid:  <http://lv2plug.in/ns/ext/urid#> .
@prefix midi:  <http://lv2plug.in/ns/ext/midi#> .

<{uri}>
    a lv2:Plugin ,
        {category} ;
    doap:name "{name}" ;
    doap:license <{license}> ;
    lv2:project <{project}> ;
    lv2:port [
"#,
        uri = P::LV2_URI,
        category = P::LV2_CATEGORY.as_uri(),
        name = P::NAME,
        license = "http://opensource.org/licenses/isc",
        project = P::URL
    );

    // Add port descriptions
    for (i, port) in port_descriptors.iter().enumerate() {
        if i > 0 {
            ttl.push_str("    ] , [\n");
        }

        let port_class = match port.port_type {
            Lv2PortType::AudioInput => "lv2:AudioPort , lv2:InputPort",
            Lv2PortType::AudioOutput => "lv2:AudioPort , lv2:OutputPort",
            Lv2PortType::ControlInput => "lv2:ControlPort , lv2:InputPort",
            Lv2PortType::ControlOutput => "lv2:ControlPort , lv2:OutputPort",
            Lv2PortType::AtomInput => "atom:AtomPort , lv2:InputPort",
            Lv2PortType::AtomOutput => "atom:AtomPort , lv2:OutputPort",
        };

        ttl.push_str(&format!(
            r#"        a {} ;
        lv2:index {} ;
        lv2:symbol "{}" ;
        lv2:name "{}"
"#,
            port_class, port.index, port.symbol, port.name
        ));

        // Add atom port specifics for MIDI
        if matches!(port.port_type, Lv2PortType::AtomInput | Lv2PortType::AtomOutput) {
            ttl.push_str(
                r#" ;
        atom:bufferType atom:Sequence ;
        atom:supports midi:MidiEvent
"#,
            );
        }
    }

    ttl.push_str("    ] .\n");
    ttl
}

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError},
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
#[cfg(target_os = "linux")]
use midir::os::unix::VirtualOutput;
use midir::{Ignore, MidiInput, MidiOutput, MidiOutputConnection};

use crate::{mpe::MpeProcessor, pitch_bend::BendRange, tuning::FiveLimitLowestRoot};

pub const OUTPUT_PORT_NAME: &str = "Adaptive JI Out";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiEvent {
    NoteOn {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
}

/// midir supplies complete messages, including the status byte (no raw running status).
pub fn parse_message(message: &[u8]) -> Option<MidiEvent> {
    let &status = message.first()?;
    let kind = status & 0xf0;
    if !matches!(kind, 0x80 | 0x90 | 0xb0) {
        log::debug!(
            "Ignoring MIDI status=0x{status:02x}, {} bytes",
            message.len()
        );
        return None;
    }
    if message.len() != 3 || message[1] >= 128 || message[2] >= 128 {
        log::debug!("Ignoring malformed MIDI message");
        return None;
    }
    let channel = status & 0x0f;
    let (first, second) = (message[1], message[2]);
    Some(match kind {
        0x80 => MidiEvent::NoteOff {
            channel,
            note: first,
            velocity: second,
        },
        0x90 if second == 0 => MidiEvent::NoteOff {
            channel,
            note: first,
            velocity: 0,
        },
        0x90 => MidiEvent::NoteOn {
            channel,
            note: first,
            velocity: second,
        },
        _ => MidiEvent::ControlChange {
            channel,
            controller: first,
            value: second,
        },
    })
}

fn input_client() -> Result<MidiInput> {
    MidiInput::new("adaptive-ji input").context(
        "Cannot initialize ALSA MIDI input. Check /dev/snd/seq, ALSA permissions and `aconnect -l`",
    )
}

pub fn list_inputs() -> Result<()> {
    let input = input_client()?;
    let ports = input.ports();
    if ports.is_empty() {
        println!("No MIDI input ports found. Connect your keyboard and run `aconnect -l`.");
    }
    for (index, port) in ports.iter().enumerate() {
        println!(
            "{index}: {}",
            input
                .port_name(port)
                .context("Cannot read MIDI input port name")?
        );
    }
    Ok(())
}

fn select_input(names: &[String], selector: &str) -> Result<usize> {
    if let Ok(index) = selector.parse::<usize>() {
        if index < names.len() {
            return Ok(index);
        }
        bail!(
            "MIDI input index {index} not found ({} ports). Run `adaptive-ji --list-inputs`",
            names.len()
        );
    }
    let exact: Vec<_> = names
        .iter()
        .enumerate()
        .filter(|(_, name)| name.as_str() == selector)
        .map(|(i, _)| i)
        .collect();
    let matches = if exact.is_empty() {
        let selector = selector.to_lowercase();
        names
            .iter()
            .enumerate()
            .filter(|(_, name)| name.to_lowercase().contains(&selector))
            .map(|(i, _)| i)
            .collect()
    } else {
        exact
    };
    match matches.as_slice() {
        [index] => Ok(*index),
        [] => bail!("MIDI input {selector:?} not found. Run `adaptive-ji --list-inputs`"),
        _ => bail!(
            "MIDI input {selector:?} is ambiguous ({} matches). Select a port index from `adaptive-ji --list-inputs`",
            matches.len()
        ),
    }
}

#[cfg(target_os = "linux")]
fn virtual_output() -> Result<MidiOutputConnection> {
    MidiOutput::new("adaptive-ji output")
        .context("Cannot initialize ALSA MIDI output")?
        .create_virtual(OUTPUT_PORT_NAME)
        .map_err(|error| anyhow!("Cannot create virtual ALSA port {OUTPUT_PORT_NAME:?}: {error}"))
}

#[cfg(not(target_os = "linux"))]
fn virtual_output() -> Result<MidiOutputConnection> {
    bail!("adaptive-ji v0.1 supports Linux/ALSA only")
}

pub fn run(selector: &str, range: BendRange) -> Result<()> {
    let mut input = input_client()?;
    input.ignore(Ignore::None);
    let ports = input.ports();
    let names = ports
        .iter()
        .map(|port| input.port_name(port))
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Cannot read MIDI input port names")?;
    let index = select_input(&names, selector)?;
    let mut output = virtual_output()?;
    let mut processor = MpeProcessor::new(FiveLimitLowestRoot, range);

    let stopping = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&stopping);
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))
        .context("Cannot install Ctrl-C handler")?;

    // The callback never waits for processing/output. Overflow is fatal: silently
    // dropping an arbitrary NoteOff could leave a sounding voice behind.
    let (sender, receiver) = mpsc::sync_channel(1024);
    let overflow = Arc::new(AtomicBool::new(false));
    let callback_overflow = Arc::clone(&overflow);
    let connection = input
        .connect(
            &ports[index],
            "adaptive-ji keyboard",
            move |_, bytes, _| {
                if let Some(event) = parse_message(bytes)
                    && sender.try_send(event).is_err()
                {
                    callback_overflow.store(true, Ordering::Relaxed);
                }
            },
            (),
        )
        .map_err(|error| anyhow!("Cannot connect MIDI input {:?}: {error}", names[index]))?;

    eprintln!(
        "{} -> {OUTPUT_PORT_NAME}; bend range ±{} semitones. Ctrl-C to stop.",
        names[index],
        range.semitones()
    );
    let result = (|| -> Result<()> {
        for message in processor.initialize() {
            output
                .send(&message)
                .context("Failed to initialize MPE channels")?;
        }
        while !stopping.load(Ordering::Relaxed) {
            if overflow.load(Ordering::Relaxed) {
                bail!(
                    "MIDI input queue overflow; stopping and releasing voices to avoid stuck notes"
                );
            }
            match receiver.recv_timeout(Duration::from_millis(20)) {
                Ok(event) => {
                    for message in processor.process(event) {
                        output
                            .send(&message)
                            .context("Failed to send MIDI output")?;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => bail!("MIDI input callback disconnected"),
            }
        }
        Ok(())
    })();

    // Stop the callback before draining state. Attempt every cleanup message even
    // when one send fails; a failed output must not prevent the other channels' reset.
    connection.close();
    let mut cleanup_error = None;
    for message in processor.shutdown() {
        if let Err(error) = output.send(&message) {
            log::error!("MIDI shutdown send failed: {error}");
            cleanup_error.get_or_insert_with(|| anyhow!("MIDI shutdown incomplete: {error}"));
        }
    }
    output.close();
    result?;
    if let Some(error) = cleanup_error {
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_notes_controllers_and_zero_velocity() {
        assert_eq!(
            parse_message(&[0x92, 60, 100]),
            Some(MidiEvent::NoteOn {
                channel: 2,
                note: 60,
                velocity: 100
            })
        );
        assert_eq!(
            parse_message(&[0x92, 60, 0]),
            Some(MidiEvent::NoteOff {
                channel: 2,
                note: 60,
                velocity: 0
            })
        );
        assert_eq!(
            parse_message(&[0x82, 60, 42]),
            Some(MidiEvent::NoteOff {
                channel: 2,
                note: 60,
                velocity: 42
            })
        );
        assert_eq!(
            parse_message(&[0xbf, 64, 127]),
            Some(MidiEvent::ControlChange {
                channel: 15,
                controller: 64,
                value: 127
            })
        );
    }

    #[test]
    fn ignores_pitch_bend_system_and_malformed_messages() {
        for bytes in [
            &[][..],
            &[0x90],
            &[0x90, 60],
            &[0x90, 128, 1],
            &[0x90, 60, 255],
            &[0xe0, 0, 64],
            &[0xf0, 1, 0xf7],
            &[0xf8],
            &[60, 100],
            &[0xd0, 10],
            &[0xc0, 1],
        ] {
            assert_eq!(parse_message(bytes), None);
        }
    }

    #[test]
    fn selects_index_exact_name_or_unique_substring() {
        let names = vec!["SMK-25 MIDI 1".into(), "Other keyboard".into()];
        assert_eq!(select_input(&names, "0").unwrap(), 0);
        assert_eq!(select_input(&names, "Other keyboard").unwrap(), 1);
        assert_eq!(select_input(&names, "smk").unwrap(), 0);
        assert!(
            select_input(&names, "2")
                .unwrap_err()
                .to_string()
                .contains("not found")
        );
        assert!(select_input(&names, "missing").is_err());
        assert!(select_input(&[], "0").is_err());
        assert!(
            select_input(&["SMK 1".into(), "SMK 2".into()], "SMK")
                .unwrap_err()
                .to_string()
                .contains("ambiguous")
        );
    }
}

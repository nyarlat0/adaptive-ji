//! LV2 Atom event handling and MIDI translation.

use std::os::raw::c_void;

use crate::prelude::{NoteEvent, PluginNoteEvent};
use crate::plugin::Plugin;

/// LV2 Atom header
#[repr(C)]
pub struct Lv2Atom {
    pub size: u32,
    pub type_: u32,
}

/// LV2 Atom Sequence header
#[repr(C)]
pub struct Lv2AtomSequence {
    pub atom: Lv2Atom,
    pub body: Lv2AtomSequenceBody,
}

/// LV2 Atom Sequence body
#[repr(C)]
pub struct Lv2AtomSequenceBody {
    pub unit: u32,
    pub pad: u32,
}

/// LV2 Atom Event (time-stamped atom in a sequence)
#[repr(C)]
pub struct Lv2AtomEvent {
    pub time: Lv2AtomEventTime,
    pub body: Lv2Atom,
}

/// LV2 Atom Event time (in frames or beats)
#[repr(C)]
pub union Lv2AtomEventTime {
    pub frames: i64,
    pub beats: f64,
}

/// LV2 MIDI event structure (atom body for MIDI events)
#[repr(C)]
pub struct Lv2MidiEvent {
    pub atom: Lv2Atom,
    pub data: [u8; 3],
}

/// Parse LV2 atom sequence and convert to NIH-plug events
pub fn parse_atom_sequence<P: Plugin>(
    atom_sequence: *const c_void,
    output_events: &mut Vec<PluginNoteEvent<P>>,
) {
    if atom_sequence.is_null() {
        return;
    }

    unsafe {
        let sequence = &*(atom_sequence as *const Lv2AtomSequence);
        let sequence_size = sequence.atom.size as usize;

        // Calculate the size of the sequence body (excluding header)
        let body_size = sequence_size.saturating_sub(std::mem::size_of::<Lv2AtomSequenceBody>());

        // Pointer to the first event in the sequence
        let mut event_ptr = (atom_sequence as *const u8)
            .add(std::mem::size_of::<Lv2AtomSequence>()) as *const Lv2AtomEvent;

        let sequence_end = (atom_sequence as *const u8).add(std::mem::size_of::<Lv2Atom>() + sequence_size);

        // Iterate through events in the sequence
        let mut offset = 0usize;
        while offset < body_size {
            if (event_ptr as *const u8) >= sequence_end {
                break;
            }

            let event = &*event_ptr;
            let timing = event.time.frames as u32; // Assuming frame-based timing

            // Get the atom body (the actual event data)
            let atom_body_ptr = (event_ptr as *const u8)
                .add(std::mem::size_of::<Lv2AtomEventTime>())
                .add(std::mem::size_of::<Lv2Atom>());

            let atom_size = event.body.size as usize;

            // Check if this is a MIDI event (we'd need to check the type_ field against MIDI URID)
            // For now, we'll assume it's MIDI if the size is 3 or 4 bytes
            if atom_size >= 3 && atom_size <= 4 {
                let midi_data = std::slice::from_raw_parts(atom_body_ptr, atom_size);
                if let Some(note_event) = midi_to_note_event(midi_data, timing) {
                    output_events.push(note_event);
                }
            }

            // Move to the next event
            // LV2 atoms are padded to 64-bit boundaries
            let event_size = std::mem::size_of::<Lv2AtomEventTime>()
                + std::mem::size_of::<Lv2Atom>()
                + atom_size;
            let padded_size = (event_size + 7) & !7; // Round up to nearest 8 bytes

            event_ptr = (event_ptr as *const u8).add(padded_size) as *const Lv2AtomEvent;
            offset += padded_size;
        }
    }
}

/// Convert MIDI data to a NIH-plug NoteEvent
fn midi_to_note_event<S>(midi_data: &[u8], _timing: u32) -> Option<NoteEvent<S>> {
    if midi_data.is_empty() {
        return None;
    }

    let status = midi_data[0];
    let message_type = status & 0xF0;
    let channel = status & 0x0F;

    match message_type {
        0x80 => {
            // Note Off
            if midi_data.len() >= 3 {
                Some(NoteEvent::NoteOff {
                    timing: 0,
                    voice_id: None,
                    channel,
                    note: midi_data[1],
                    velocity: midi_data[2] as f32 / 127.0,
                })
            } else {
                None
            }
        }
        0x90 => {
            // Note On
            if midi_data.len() >= 3 {
                let velocity = midi_data[2] as f32 / 127.0;
                if velocity == 0.0 {
                    // Note on with velocity 0 is treated as note off
                    Some(NoteEvent::NoteOff {
                        timing: 0,
                        voice_id: None,
                        channel,
                        note: midi_data[1],
                        velocity: 0.0,
                    })
                } else {
                    Some(NoteEvent::NoteOn {
                        timing: 0,
                        voice_id: None,
                        channel,
                        note: midi_data[1],
                        velocity,
                    })
                }
            } else {
                None
            }
        }
        0xA0 => {
            // Polyphonic Aftertouch
            if midi_data.len() >= 3 {
                Some(NoteEvent::PolyPressure {
                    timing: 0,
                    voice_id: None,
                    channel,
                    note: midi_data[1],
                    pressure: midi_data[2] as f32 / 127.0,
                })
            } else {
                None
            }
        }
        0xB0 => {
            // Control Change
            if midi_data.len() >= 3 {
                Some(NoteEvent::MidiCC {
                    timing: 0,
                    channel,
                    cc: midi_data[1],
                    value: midi_data[2] as f32 / 127.0,
                })
            } else {
                None
            }
        }
        0xD0 => {
            // Channel Aftertouch
            if midi_data.len() >= 2 {
                Some(NoteEvent::MidiChannelPressure {
                    timing: 0,
                    channel,
                    pressure: midi_data[1] as f32 / 127.0,
                })
            } else {
                None
            }
        }
        0xE0 => {
            // Pitch Bend
            if midi_data.len() >= 3 {
                let value = ((midi_data[2] as u16) << 7) | (midi_data[1] as u16);
                let normalized = (value as f32 - 8192.0) / 8192.0;
                Some(NoteEvent::MidiPitchBend {
                    timing: 0,
                    channel,
                    value: normalized,
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Write NIH-plug events to an LV2 atom sequence
pub fn write_atom_sequence<P: Plugin>(
    events: &[PluginNoteEvent<P>],
    atom_sequence: *mut c_void,
    capacity: usize,
) {
    if atom_sequence.is_null() || events.is_empty() {
        return;
    }

    unsafe {
        let sequence = &mut *(atom_sequence as *mut Lv2AtomSequence);

        // Initialize the sequence header
        sequence.atom.size = std::mem::size_of::<Lv2AtomSequenceBody>() as u32;
        // sequence.atom.type_ would be set to the Atom Sequence URID
        sequence.body.unit = 0; // Frame units
        sequence.body.pad = 0;

        let mut write_ptr = (atom_sequence as *mut u8)
            .add(std::mem::size_of::<Lv2AtomSequence>());

        let sequence_end = (atom_sequence as *mut u8).add(capacity);

        for event in events {
            // Get timing and convert the event to MIDI
            let timing = match event {
                NoteEvent::NoteOn { timing, .. }
                | NoteEvent::NoteOff { timing, .. }
                | NoteEvent::PolyPressure { timing, .. }
                | NoteEvent::MidiCC { timing, .. }
                | NoteEvent::MidiChannelPressure { timing, .. }
                | NoteEvent::MidiPitchBend { timing, .. } => *timing,
                _ => 0,
            };

            let channel = match event {
                NoteEvent::NoteOn { channel, .. }
                | NoteEvent::NoteOff { channel, .. }
                | NoteEvent::PolyPressure { channel, .. }
                | NoteEvent::MidiCC { channel, .. }
                | NoteEvent::MidiChannelPressure { channel, .. }
                | NoteEvent::MidiPitchBend { channel, .. } => *channel,
                _ => 0,
            };

            if let Some(midi_data) = note_event_to_midi(event, channel) {
                    let event_size = std::mem::size_of::<Lv2AtomEventTime>()
                        + std::mem::size_of::<Lv2Atom>()
                        + midi_data.len();
                    let padded_size = (event_size + 7) & !7;

                    // Check if we have enough space
                    if write_ptr.add(padded_size) > sequence_end {
                        break;
                    }

                    // Write the event
                    let event_ptr = write_ptr as *mut Lv2AtomEvent;
                    (*event_ptr).time.frames = timing as i64;
                    (*event_ptr).body.size = midi_data.len() as u32;
                    // (*event_ptr).body.type_ would be set to the MIDI URID

                    // Write MIDI data
                    let midi_ptr = write_ptr
                        .add(std::mem::size_of::<Lv2AtomEventTime>())
                        .add(std::mem::size_of::<Lv2Atom>());
                    std::ptr::copy_nonoverlapping(midi_data.as_ptr(), midi_ptr, midi_data.len());

                    // Update sequence size
                    sequence.atom.size += padded_size as u32;
                    write_ptr = write_ptr.add(padded_size);
                }
            }
    }
}

/// Convert a NIH-plug NoteEvent to MIDI data
fn note_event_to_midi<S>(event: &NoteEvent<S>, channel: u8) -> Option<Vec<u8>> {
    let channel = channel & 0x0F;

    match event {
        NoteEvent::NoteOn {
            note, velocity, ..
        } => {
            let vel = (*velocity * 127.0).round() as u8;
            Some(vec![0x90 | channel, *note, vel])
        }
        NoteEvent::NoteOff {
            note, velocity, ..
        } => {
            let vel = (*velocity * 127.0).round() as u8;
            Some(vec![0x80 | channel, *note, vel])
        }
        NoteEvent::PolyPressure {
            note, pressure, ..
        } => {
            let press = (*pressure * 127.0).round() as u8;
            Some(vec![0xA0 | channel, *note, press])
        }
        NoteEvent::MidiCC { cc, value, .. } => {
            let val = (*value * 127.0).round() as u8;
            Some(vec![0xB0 | channel, *cc, val])
        }
        NoteEvent::MidiChannelPressure { pressure, .. } => {
            let press = (*pressure * 127.0).round() as u8;
            Some(vec![0xD0 | channel, press])
        }
        NoteEvent::MidiPitchBend { value, .. } => {
            let bend_value = ((*value * 8192.0) + 8192.0).round() as u16;
            let lsb = (bend_value & 0x7F) as u8;
            let msb = ((bend_value >> 7) & 0x7F) as u8;
            Some(vec![0xE0 | channel, lsb, msb])
        }
        _ => None,
    }
}

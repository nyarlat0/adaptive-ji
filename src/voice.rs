use std::collections::{BTreeMap, VecDeque};

use crate::tuning::VoiceId;

#[derive(Clone, Debug)]
pub struct Voice {
    pub id: VoiceId,
    pub input_note: u8,
    pub velocity: u8,
    /// Zero-based MIDI wire channel: 1..=15 (displayed as 2..=16).
    pub output_channel: u8,
    pub cents_offset: f64,
}

#[derive(Default)]
pub struct VoiceAllocator {
    voices: Vec<Voice>,
    next_id: VoiceId,
    // None records an ignored NoteOn, so its eventual NoteOff cannot kill a later voice.
    pending: BTreeMap<(u8, u8), VecDeque<Option<VoiceId>>>,
}

impl VoiceAllocator {
    pub fn voices(&self) -> &[Voice] {
        &self.voices
    }

    pub fn voice_mut(&mut self, id: VoiceId) -> Option<&mut Voice> {
        self.voices.iter_mut().find(|voice| voice.id == id)
    }

    pub fn allocate(&mut self, input_channel: u8, input_note: u8, velocity: u8) -> Option<Voice> {
        let channel = (1..=15).find(|channel| {
            self.voices
                .iter()
                .all(|voice| voice.output_channel != *channel)
        });
        let pending = self.pending.entry((input_channel, input_note)).or_default();
        let Some(output_channel) = channel else {
            pending.push_back(None);
            return None;
        };
        self.next_id += 1;
        let voice = Voice {
            id: self.next_id,
            input_note,
            velocity,
            output_channel,
            cents_offset: 0.0,
        };
        pending.push_back(Some(voice.id));
        self.voices.push(voice.clone());
        Some(voice)
    }

    /// MIDI 1.0 has no note IDs; repeated keys are matched oldest-first per input channel.
    pub fn release(&mut self, input_channel: u8, input_note: u8) -> Option<Voice> {
        let key = (input_channel, input_note);
        let pending = self.pending.get_mut(&key)?;
        let id = pending.pop_front().flatten();
        if pending.is_empty() {
            self.pending.remove(&key);
        }
        let id = id?;
        let index = self.voices.iter().position(|voice| voice.id == id)?;
        Some(self.voices.remove(index))
    }

    pub fn clear(&mut self) {
        self.voices.clear();
        self.pending.clear();
    }
}

pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(note % 12)],
        i16::from(note / 12) - 1
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_fifteen_members_and_reuses_released_channel() {
        let mut allocator = VoiceAllocator::default();
        for i in 0..15 {
            assert_eq!(
                allocator.allocate(0, 60 + i, 100).unwrap().output_channel,
                i + 1
            );
        }
        assert!(allocator.allocate(0, 90, 100).is_none());
        assert_eq!(allocator.voices().len(), 15);
        let released = allocator.release(0, 65).unwrap();
        assert_eq!(released.output_channel, 6);
        assert_eq!(allocator.allocate(0, 91, 100).unwrap().output_channel, 6);
        assert!(allocator.release(0, 90).is_none());
    }

    #[test]
    fn repeated_keys_are_fifo_and_input_channels_are_independent() {
        let mut allocator = VoiceAllocator::default();
        let first = allocator.allocate(0, 60, 80).unwrap();
        let second = allocator.allocate(0, 60, 90).unwrap();
        let other = allocator.allocate(1, 60, 100).unwrap();
        assert_ne!(first.output_channel, second.output_channel);
        assert_eq!(allocator.release(1, 60).unwrap().id, other.id);
        assert_eq!(allocator.release(0, 60).unwrap().id, first.id);
        assert_eq!(allocator.release(0, 60).unwrap().id, second.id);
        assert!(allocator.release(0, 60).is_none());
    }

    #[test]
    fn rejected_duplicate_off_does_not_release_later_voice() {
        let mut allocator = VoiceAllocator::default();
        for note in 60..75 {
            allocator.allocate(0, note, 100).unwrap();
        }
        assert!(allocator.allocate(0, 60, 100).is_none());
        allocator.release(0, 60).unwrap();
        let later = allocator.allocate(0, 60, 100).unwrap();
        assert!(allocator.release(0, 60).is_none());
        assert!(allocator.voices().iter().any(|voice| voice.id == later.id));
        assert_eq!(allocator.release(0, 60).unwrap().id, later.id);
    }
}

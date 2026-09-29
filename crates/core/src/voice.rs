use crate::tuning::VoiceId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VoiceState {
    Held,
    ReleasedSustained,
}

#[derive(Clone, Copy, Debug)]
pub struct Voice {
    pub id: VoiceId,
    pub input_note: u8,
    pub output_note: u8,
    pub velocity: u8,
    /// Zero-based MIDI wire channel: 1..=15 (displayed as 2..=16).
    pub output_channel: u8,
    pub cents_offset: f64,
    pub state: VoiceState,
    input_channel: u8,
    key_sequence: u64,
}

#[derive(Clone, Debug)]
pub enum KeyRelease {
    Released(Voice),
    Sustained(Voice),
}

pub struct VoiceAllocator {
    voices: Vec<Voice>,
    next_id: VoiceId,
    // Per-key sequence gaps represent ignored NoteOns without an unbounded FIFO.
    // A live voice remembers its ordinal. No per-event allocation is necessary.
    issued: Box<[u64; 16 * 128]>,
    released: Box<[u64; 16 * 128]>,
}

impl Default for VoiceAllocator {
    fn default() -> Self {
        Self {
            voices: Vec::with_capacity(15),
            next_id: 0,
            issued: Box::new([0; 16 * 128]),
            released: Box::new([0; 16 * 128]),
        }
    }
}

impl VoiceAllocator {
    /// Preserve FIFO NoteOff pairing for a key intentionally left unmapped.
    pub fn ignore_note_on(&mut self, input_channel: u8, input_note: u8) {
        let key = usize::from(input_channel) * 128 + usize::from(input_note);
        self.issued[key] = self.issued[key].saturating_add(1);
    }

    pub fn voices(&self) -> &[Voice] {
        &self.voices
    }

    pub fn voice_mut(&mut self, id: VoiceId) -> Option<&mut Voice> {
        self.voices.iter_mut().find(|voice| voice.id == id)
    }

    pub fn allocate(&mut self, input_channel: u8, input_note: u8, velocity: u8) -> Option<Voice> {
        self.allocate_mapped(input_channel, input_note, input_note, velocity)
    }

    pub fn allocate_mapped(
        &mut self,
        input_channel: u8,
        input_note: u8,
        output_note: u8,
        velocity: u8,
    ) -> Option<Voice> {
        let channel = (1..=15).find(|channel| {
            self.voices
                .iter()
                .all(|voice| voice.output_channel != *channel)
        });
        self.ignore_note_on(input_channel, input_note);
        let output_channel = channel?;
        self.next_id = self.next_id.checked_add(1)?;
        let voice = Voice {
            id: self.next_id,
            input_note,
            output_note,
            velocity,
            output_channel,
            cents_offset: 0.0,
            state: VoiceState::Held,
            input_channel,
            key_sequence: self.issued[usize::from(input_channel) * 128 + usize::from(input_note)],
        };
        self.voices.push(voice);
        Some(voice)
    }

    /// MIDI 1.0 has no note IDs; repeated keys are matched oldest-first per input channel.
    /// Inspect the matching voice without consuming its FIFO entry. The output
    /// layer must send its mapped NoteOff before committing the release.
    pub fn next_release(&self, input_channel: u8, input_note: u8) -> Option<&Voice> {
        let key = usize::from(input_channel) * 128 + usize::from(input_note);
        if self.released[key] == self.issued[key] {
            return None;
        }
        let ordinal = self.released[key] + 1;
        self.voices.iter().find(|voice| {
            voice.input_channel == input_channel
                && voice.input_note == input_note
                && voice.key_sequence == ordinal
        })
    }

    /// Commit a physical key release after its output NoteOff has been sent.
    pub fn release_key(
        &mut self,
        input_channel: u8,
        input_note: u8,
        sustain_down: bool,
    ) -> Option<KeyRelease> {
        let key = usize::from(input_channel) * 128 + usize::from(input_note);
        if self.released[key] == self.issued[key] {
            return None;
        }
        let id = self
            .next_release(input_channel, input_note)
            .map(|voice| voice.id);
        self.released[key] += 1;
        let id = id?;
        let index = self.voices.iter().position(|voice| voice.id == id)?;
        if sustain_down {
            let voice = &mut self.voices[index];
            voice.state = VoiceState::ReleasedSustained;
            Some(KeyRelease::Sustained(*voice))
        } else {
            Some(KeyRelease::Released(self.voices.remove(index)))
        }
    }

    /// Remove voices whose keys were released while CC64 was down.
    pub fn release_sustained(&mut self) -> Vec<Voice> {
        let mut released = Vec::new();
        self.voices.retain(|voice| {
            if voice.state == VoiceState::ReleasedSustained {
                released.push(*voice);
                false
            } else {
                true
            }
        });
        released
    }

    pub fn state_counts(&self) -> (usize, usize) {
        let held = self
            .voices
            .iter()
            .filter(|voice| voice.state == VoiceState::Held)
            .count();
        (held, self.voices.len() - held)
    }

    pub fn remove_sustained(&mut self) {
        self.voices
            .retain(|voice| voice.state != VoiceState::ReleasedSustained);
    }

    /// Immediate release, retained for callers that do not model sustain.
    #[cfg(test)]
    pub fn release(&mut self, input_channel: u8, input_note: u8) -> Option<Voice> {
        match self.release_key(input_channel, input_note, false)? {
            KeyRelease::Released(voice) => Some(voice),
            KeyRelease::Sustained(_) => unreachable!(),
        }
    }

    pub fn clear(&mut self) {
        self.voices.clear();
        self.issued.fill(0);
        self.released.fill(0);
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

    #[test]
    fn sustained_restrikes_keep_distinct_ids_and_channels() {
        let mut allocator = VoiceAllocator::default();
        let first = allocator.allocate(0, 60, 80).unwrap();
        assert!(matches!(
            allocator.release_key(0, 60, true),
            Some(KeyRelease::Sustained(voice)) if voice.id == first.id
        ));
        let second = allocator.allocate(0, 60, 90).unwrap();
        assert_ne!(first.id, second.id);
        assert_ne!(first.output_channel, second.output_channel);
        assert_eq!(allocator.state_counts(), (1, 1));
        assert!(matches!(
            allocator.release_key(0, 60, true),
            Some(KeyRelease::Sustained(voice)) if voice.id == second.id
        ));
        assert_eq!(allocator.state_counts(), (0, 2));
        let released = allocator.release_sustained();
        assert_eq!(
            released.iter().map(|v| v.id).collect::<Vec<_>>(),
            vec![first.id, second.id]
        );
        assert!(allocator.voices().is_empty());
        assert_eq!(allocator.allocate(0, 60, 100).unwrap().output_channel, 1);
    }

    #[test]
    fn all_sustained_voices_reserve_channels_until_pedal_up() {
        let mut allocator = VoiceAllocator::default();
        for _ in 0..15 {
            allocator.allocate(0, 60, 100).unwrap();
            allocator.release_key(0, 60, true).unwrap();
        }
        assert_eq!(allocator.state_counts(), (0, 15));
        assert!(allocator.allocate(0, 61, 100).is_none());
        assert_eq!(allocator.release_sustained().len(), 15);
        assert_eq!(allocator.allocate(0, 62, 100).unwrap().output_channel, 1);
    }

    #[test]
    fn pedal_up_keeps_physically_held_voices() {
        let mut allocator = VoiceAllocator::default();
        let sustained = allocator.allocate(0, 60, 100).unwrap();
        allocator.release_key(0, 60, true).unwrap();
        let held = allocator.allocate(0, 64, 100).unwrap();
        assert_eq!(allocator.release_sustained()[0].id, sustained.id);
        assert_eq!(allocator.voices()[0].id, held.id);
        assert_eq!(allocator.voices()[0].state, VoiceState::Held);
    }
}

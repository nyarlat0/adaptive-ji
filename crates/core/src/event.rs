/// Backend-independent MIDI 1.0 events. Channels use zero-based wire numbering.
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

/// Bounded per-event scratch, never allocates. The largest MPE event expansion
/// (controller reset with 15 held voices) is below this capacity.
pub(crate) struct EventBuffer {
    messages: [[u8; 3]; 256],
    len: usize,
}

impl EventBuffer {
    pub fn new() -> Self {
        Self {
            messages: [[0; 3]; 256],
            len: 0,
        }
    }
    pub fn push(&mut self, message: [u8; 3]) {
        assert!(
            self.len < self.messages.len(),
            "bounded MPE event expansion"
        );
        self.messages[self.len] = message;
        self.len += 1;
    }
    pub fn extend(&mut self, events: impl IntoIterator<Item = [u8; 3]>) {
        for event in events {
            self.push(event);
        }
    }
}

impl IntoIterator for EventBuffer {
    type Item = [u8; 3];
    type IntoIter = std::iter::Take<std::array::IntoIter<[u8; 3], 256>>;
    fn into_iter(self) -> Self::IntoIter {
        self.messages.into_iter().take(self.len)
    }
}

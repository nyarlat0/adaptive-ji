use crate::{
    midi::MidiEvent,
    pitch_bend::{BendRange, CENTER, cents_to_pitch_bend, encode_pitch_bend},
    tuning::{ActiveNote, TuningStrategy},
    voice::{Voice, VoiceAllocator, note_name},
};

pub type MidiMessage = [u8; 3];

/// MIDI-independent state machine: the transport only sends the returned messages in order.
pub struct MpeProcessor<T> {
    tuning: T,
    allocator: VoiceAllocator,
    bend_range: BendRange,
}

impl<T: TuningStrategy> MpeProcessor<T> {
    pub fn new(tuning: T, bend_range: BendRange) -> Self {
        Self {
            tuning,
            allocator: VoiceAllocator::default(),
            bend_range,
        }
    }

    pub fn voices(&self) -> &[Voice] {
        self.allocator.voices()
    }

    pub fn initialize(&self) -> Vec<MidiMessage> {
        let mut output = Vec::with_capacity(15 * 7);
        for channel in 1..=15 {
            output.extend(self.bend_range.rpn(channel));
            output.push(encode_pitch_bend(channel, CENTER));
        }
        output
    }

    pub fn process(&mut self, event: MidiEvent) -> Vec<MidiMessage> {
        let mut output = Vec::with_capacity(32);
        match event {
            MidiEvent::NoteOn {
                channel,
                note,
                velocity: 0,
            }
            | MidiEvent::NoteOff {
                channel,
                note,
                velocity: 0,
            } => {
                self.note_off(channel, note, 0, &mut output);
            }
            MidiEvent::NoteOn {
                channel,
                note,
                velocity,
            } => {
                let Some(voice) = self.allocator.allocate(channel, note, velocity) else {
                    log::warn!(
                        "No free MPE channel (15 voices); ignoring NoteOn {} input={note}",
                        note_name(note)
                    );
                    return output;
                };
                log::info!(
                    "NOTE ON  {:<4} input={note} voice={} ch={}",
                    note_name(note),
                    voice.id,
                    voice.output_channel + 1
                );
                // Virtual ports may be connected after startup, so repeat RPN at allocation.
                output.extend(self.bend_range.rpn(voice.output_channel));
                self.retune(&mut output);
                output.push([0x90 | voice.output_channel, note, velocity]);
            }
            MidiEvent::NoteOff {
                channel,
                note,
                velocity,
            } => {
                self.note_off(channel, note, velocity, &mut output);
            }
            MidiEvent::ControlChange {
                controller, value, ..
            } => {
                // Explicit panic controls must also clear allocator state.
                if matches!(controller, 120 | 123) {
                    output.extend(self.shutdown());
                }
                // Includes CC64: free channels also receive pedal state, and pedal-up
                // reaches channels whose voices have already been released.
                output.extend((1..=15).map(|channel| [0xb0 | channel, controller, value]));
            }
        }
        output
    }

    fn note_off(&mut self, channel: u8, note: u8, velocity: u8, output: &mut Vec<MidiMessage>) {
        let Some(voice) = self.allocator.release(channel, note) else {
            log::debug!("Ignored NoteOff input={note} input_ch={}", channel + 1);
            return;
        };
        log::info!(
            "NOTE OFF {:<4} voice={} ch={}",
            note_name(note),
            voice.id,
            voice.output_channel + 1
        );
        output.push([0x80 | voice.output_channel, note, velocity]);
        output.push(encode_pitch_bend(voice.output_channel, CENTER));
        self.retune(output);
    }

    fn retune(&mut self, output: &mut Vec<MidiMessage>) {
        let notes: Vec<_> = self
            .allocator
            .voices()
            .iter()
            .map(|voice| ActiveNote {
                id: voice.id,
                input_note: voice.input_note,
            })
            .collect();
        for tuned in self.tuning.tune(&notes) {
            if let Some(voice) = self.allocator.voice_mut(tuned.id) {
                voice.cents_offset = tuned.cents_offset;
                let bend = cents_to_pitch_bend(tuned.cents_offset, self.bend_range.semitones());
                output.push(encode_pitch_bend(voice.output_channel, bend));
                log::info!(
                    "TUNE     {:<4} {:<7} {:+.3}c  bend={bend}",
                    note_name(voice.input_note),
                    tuned.ratio.to_string(),
                    tuned.cents_offset
                );
            }
        }
    }

    pub fn shutdown(&mut self) -> Vec<MidiMessage> {
        let mut output = Vec::with_capacity(62);
        for voice in self.allocator.voices() {
            output.push([0x80 | voice.output_channel, voice.input_note, 0]);
        }
        for channel in 0..=15 {
            // All Notes Off alone does not silence sustain-held notes.
            output.push([0xb0 | channel, 64, 0]);
            output.push([0xb0 | channel, 123, 0]);
        }
        for channel in 1..=15 {
            output.push(encode_pitch_bend(channel, CENTER));
        }
        self.allocator.clear();
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::FiveLimitLowestRoot;

    fn processor() -> MpeProcessor<FiveLimitLowestRoot> {
        MpeProcessor::new(FiveLimitLowestRoot, "2".parse().unwrap())
    }

    fn on(note: u8) -> MidiEvent {
        MidiEvent::NoteOn {
            channel: 0,
            note,
            velocity: 100,
        }
    }

    fn off(note: u8) -> MidiEvent {
        MidiEvent::NoteOff {
            channel: 0,
            note,
            velocity: 45,
        }
    }

    #[test]
    fn sends_bend_before_note_on_and_retunes_without_retrigger() {
        let mut processor = processor();
        let output = processor.process(on(60));
        assert_eq!(
            &output[output.len() - 2..],
            &[[0xe1, 0, 64], [0x91, 60, 100]]
        );
        let output = processor.process(on(64));
        assert!(output.contains(&encode_pitch_bend(2, 7631)));
        assert_eq!(output.iter().filter(|msg| msg[0] & 0xf0 == 0x90).count(), 1);
        assert_eq!(output.last(), Some(&[0x92, 64, 100]));
        let output = processor.process(on(67));
        assert!(output.contains(&encode_pitch_bend(3, 8272)));
        assert_eq!(output.iter().filter(|msg| msg[0] & 0xf0 == 0xe0).count(), 3);
    }

    #[test]
    fn adding_and_removing_root_retunes_held_voices() {
        let mut processor = processor();
        processor.process(on(64));
        processor.process(on(67));
        let output = processor.process(on(60));
        assert!(output.contains(&encode_pitch_bend(1, 7631)));
        let output = processor.process(off(60));
        assert_eq!(output[0], [0x83, 60, 45]);
        assert_eq!(output[1], encode_pitch_bend(3, CENTER));
        assert!(output.contains(&encode_pitch_bend(1, CENTER)));
        assert!(!output.iter().any(|msg| msg[0] & 0xf0 == 0x90));
        assert_eq!(processor.voices()[0].cents_offset, 0.0);
        assert!((processor.voices()[1].cents_offset - 15.641287).abs() < 1e-6);
    }

    #[test]
    fn repeated_notes_and_zero_velocity_off() {
        let mut processor = processor();
        processor.process(on(60));
        processor.process(on(60));
        let output = processor.process(MidiEvent::NoteOn {
            channel: 0,
            note: 60,
            velocity: 0,
        });
        assert_eq!(output[0], [0x81, 60, 0]);
        assert_eq!(processor.voices().len(), 1);
        assert_eq!(processor.voices()[0].output_channel, 2);
        assert_eq!(processor.process(off(60))[0], [0x82, 60, 45]);
        assert!(processor.voices().is_empty());
        assert!(processor.process(off(60)).is_empty());
    }

    #[test]
    fn exhaustion_sends_nothing_and_keeps_existing_voices() {
        let mut processor = processor();
        for note in 60..75 {
            processor.process(on(note));
        }
        let ids: Vec<_> = processor.voices().iter().map(|v| v.id).collect();
        assert!(processor.process(on(30)).is_empty());
        assert!(processor.process(off(30)).is_empty());
        assert_eq!(
            ids,
            processor.voices().iter().map(|v| v.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn broadcasts_controllers_including_pedal_up_on_free_channels() {
        let mut processor = processor();
        for (controller, value) in [(1, 42), (64, 127), (64, 0)] {
            let output = processor.process(MidiEvent::ControlChange {
                channel: 0,
                controller,
                value,
            });
            assert_eq!(
                output,
                (1..=15)
                    .map(|ch| [0xb0 | ch, controller, value])
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn initialization_and_shutdown_cover_all_channels() {
        let mut processor = processor();
        let init = processor.initialize();
        assert_eq!(init.len(), 105);
        for channel in 1..=15 {
            assert!(init.contains(&[0xb0 | channel, 6, 2]));
            assert!(init.contains(&encode_pitch_bend(channel, CENTER)));
        }
        processor.process(on(60));
        processor.process(on(64));
        let output = processor.shutdown();
        assert_eq!(&output[..2], &[[0x81, 60, 0], [0x82, 64, 0]]);
        for channel in 0..=15 {
            assert!(output.contains(&[0xb0 | channel, 64, 0]));
            assert!(output.contains(&[0xb0 | channel, 123, 0]));
        }
        for channel in 1..=15 {
            assert!(output.contains(&encode_pitch_bend(channel, CENTER)));
        }
        assert!(processor.voices().is_empty());
        assert!(processor.process(off(60)).is_empty());
    }

    #[test]
    fn panic_controllers_clear_voice_state() {
        for controller in [120, 123] {
            let mut processor = processor();
            processor.process(on(60));
            let output = processor.process(MidiEvent::ControlChange {
                channel: 0,
                controller,
                value: 0,
            });
            assert!(processor.voices().is_empty());
            assert!(output.contains(&[0xb1, controller, 0]));
        }
    }
}

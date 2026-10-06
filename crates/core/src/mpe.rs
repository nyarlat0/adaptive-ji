use std::{convert::Infallible, time::Duration};

use crate::{
    event::{EventBuffer, MidiEvent},
    glide::{GlideConfig, GlideEngine, TargetUpdate},
    pitch_bend::{BendRange, CENTER, cents_to_pitch_bend, encode_pitch_bend},
    tuning::{ActiveNote, TunedNote, TuningStrategy},
    voice::{Voice, VoiceAllocator, VoiceState, note_name},
};

pub type MidiMessage = [u8; 3];

macro_rules! diagnostic {
    ($processor:ident, $level:ident, $($arg:tt)*) => {
        if $processor.diagnostics { log::$level!($($arg)*); }
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanicKind {
    AllSoundOff,
    AllNotesOff,
    Shutdown,
    /// Automatic plugin lifecycle reset. An empty engine needs state reset,
    /// not an unsolicited All Sound Off next to the first incoming NoteOn.
    /// Explicit panic/shutdown always broadcasts, even with no known voices.
    HostReset,
}

/// MIDI-independent state machine with a fallible output sink for the live
/// transport and a collecting sink for deterministic tests/embedders.
pub struct MpeProcessor<T> {
    tuning: T,
    allocator: VoiceAllocator,
    bend_range: BendRange,
    sustain_down: bool,
    glide: GlideEngine,
    tuned_scratch: Vec<TunedNote>,
    diagnostics: bool,
    defer_attacks: bool,
    pending_attacks: [u64; 15],
}

impl<T: TuningStrategy> MpeProcessor<T> {
    pub fn new(tuning: T, bend_range: BendRange) -> Self {
        // Preserve the immediate-output API for existing embedders/tests.
        Self::with_glide(
            tuning,
            bend_range,
            GlideConfig {
                duration_ms: 0,
                tick_ms: 5,
            },
        )
    }

    pub fn with_glide(tuning: T, bend_range: BendRange, glide_config: GlideConfig) -> Self {
        Self {
            tuning,
            allocator: VoiceAllocator::default(),
            bend_range,
            sustain_down: false,
            glide: GlideEngine::new(glide_config, bend_range.semitones()),
            tuned_scratch: Vec::with_capacity(15),
            diagnostics: true,
            defer_attacks: false,
            pending_attacks: [0; 15],
        }
    }

    pub fn voices(&self) -> &[Voice] {
        self.allocator.voices()
    }

    pub fn set_diagnostics(&mut self, enabled: bool) {
        self.diagnostics = enabled;
    }
    pub fn use_sample_clock(&mut self, sample_rate: f64) {
        self.glide.use_sample_clock(sample_rate);
    }
    pub fn strategy_mut(&mut self) -> &mut T {
        &mut self.tuning
    }

    /// Configure only while empty. Standalone retains its synchronous path.
    pub fn defer_attacks(&mut self, enabled: bool) {
        assert!(self.voices().is_empty());
        self.defer_attacks = enabled;
    }

    pub fn is_pending(&self, id: u64) -> bool {
        self.pending_attacks.contains(&id)
    }

    fn forget_pending(&mut self, id: u64) {
        for slot in &mut self.pending_attacks {
            if *slot == id {
                *slot = 0;
            }
        }
    }

    /// Called only after accepting a complete, current-generation solution.
    /// No pending voice has glide state, so retune emits its target immediately.
    pub fn start_pending_to(&mut self, now: Duration, mut send: impl FnMut(MidiMessage)) {
        let pending = self.pending_attacks;
        for voice in self.voices().iter().filter(|v| pending.contains(&v.id)) {
            for message in self.bend_range.rpn(voice.output_channel) {
                send(message);
            }
        }
        self.pending_attacks.fill(0);
        self.retune_to(now, &mut send);
        for voice in self.voices().iter().filter(|v| pending.contains(&v.id)) {
            send([
                0x90 | voice.output_channel,
                voice.output_note,
                voice.velocity,
            ]);
            // Key released while waiting, but the pedal still holds this voice.
            if voice.state == VoiceState::ReleasedSustained {
                send([0x80 | voice.output_channel, voice.output_note, 0]);
            }
        }
    }

    pub fn snapshot(&self, now: Duration, output: &mut [ActiveNote; 15]) -> usize {
        for (slot, voice) in output.iter_mut().zip(self.allocator.voices()) {
            *slot = ActiveNote {
                id: voice.id,
                input_note: voice.input_note,
                current_offset_cents: self.glide.current_offset_at(voice.id, now).map(|emitted| {
                    emitted + 100.0 * (f64::from(voice.output_note) - f64::from(voice.input_note))
                }),
            };
        }
        self.allocator.voices().len()
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
        self.process_at(event, Duration::ZERO)
    }

    pub fn process_at(&mut self, event: MidiEvent, now: Duration) -> Vec<MidiMessage> {
        // In-memory sink for embedders/tests. The live transport uses
        // process_at_to so releases are committed only after real send calls.
        let mut output = Vec::with_capacity(32);
        self.process_at_to(event, now, |message| {
            output.push(*message);
            Ok::<_, Infallible>(())
        })
        .unwrap();
        output
    }

    /// Deliver one event in order. A failed send retains any not-yet-terminated
    /// voice identities for the transport's fatal panic/retry path.
    pub fn process_at_to<E>(
        &mut self,
        event: MidiEvent,
        now: Duration,
        mut send: impl FnMut(&MidiMessage) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut output = EventBuffer::new();
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
                return self.note_off(channel, note, 0, now, &mut send);
            }
            MidiEvent::NoteOn {
                channel,
                note,
                velocity,
            } => {
                if !self.tuning.accepts(note) {
                    self.allocator.ignore_note_on(channel, note);
                    diagnostic!(
                        self,
                        warn,
                        "NoteOn ignored: {} input={note} is unmapped or outside the Scala output range; see --scale-info",
                        note_name(note)
                    );
                    return Ok(());
                }
                let output_note = self.tuning.output_note(note);
                let Some(voice) =
                    self.allocator
                        .allocate_mapped(channel, note, output_note, velocity)
                else {
                    diagnostic!(
                        self,
                        warn,
                        "No free MPE channel (15 voices); ignoring NoteOn {} input={note}",
                        note_name(note)
                    );
                    return Ok(());
                };
                if self.defer_attacks {
                    *self
                        .pending_attacks
                        .iter_mut()
                        .find(|id| **id == 0)
                        .unwrap() = voice.id;
                    return Ok(());
                }
                diagnostic!(
                    self,
                    info,
                    "NOTE ON  {:<4} input={note} voice={} ch={}",
                    note_name(note),
                    voice.id,
                    voice.output_channel + 1
                );
                // Virtual ports may be connected after startup, so repeat RPN at allocation.
                output.extend(self.bend_range.rpn(voice.output_channel));
                self.retune(now, &mut output);
                output.push([0x90 | voice.output_channel, voice.output_note, velocity]);
            }
            MidiEvent::NoteOff {
                channel,
                note,
                velocity,
            } => {
                return self.note_off(channel, note, velocity, now, &mut send);
            }
            MidiEvent::ControlChange {
                controller, value, ..
            } => {
                match controller {
                    120 => return self.panic_output(PanicKind::AllSoundOff, send),
                    123 => return self.panic_output(PanicKind::AllNotesOff, send),
                    121 => return self.release_pedal(0, true, now, &mut send),
                    64 if value < 64 => return self.release_pedal(value, false, now, &mut send),
                    64 => {
                        for channel in 1..=15 {
                            send(&[0xb0 | channel, 64, value])?;
                        }
                        if !self.sustain_down {
                            diagnostic!(self, info, "PEDAL DOWN");
                        }
                        self.sustain_down = true;
                        return Ok(());
                    }
                    _ => {}
                }
                // Free member channels receive other controllers too.
                output.extend((1..=15).map(|channel| [0xb0 | channel, controller, value]));
            }
        }
        for message in output {
            send(&message)?;
        }
        Ok(())
    }

    fn note_off<E>(
        &mut self,
        channel: u8,
        note: u8,
        velocity: u8,
        now: Duration,
        send: &mut impl FnMut(&MidiMessage) -> Result<(), E>,
    ) -> Result<(), E> {
        let Some(voice) = self.allocator.next_release(channel, note).cloned() else {
            // Consume an ignored NoteOn's FIFO placeholder as before.
            self.allocator.release_key(channel, note, self.sustain_down);
            diagnostic!(
                self,
                debug,
                "Ignored NoteOff input={note} input_ch={}",
                channel + 1
            );
            return Ok(());
        };
        if !self.is_pending(voice.id) {
            send(&[0x80 | voice.output_channel, voice.output_note, velocity])?;
        }
        let sustained = self.sustain_down;
        self.allocator.release_key(channel, note, sustained);
        diagnostic!(
            self,
            info,
            "NOTE OFF {:<4} voice={} ch={}{}",
            note_name(note),
            voice.id,
            voice.output_channel + 1,
            if sustained { " sustained" } else { "" }
        );
        if !sustained {
            self.forget_pending(voice.id);
            self.glide.remove(voice.id);
            send(&encode_pitch_bend(voice.output_channel, CENTER))?;
            let mut output = EventBuffer::new();
            self.retune(now, &mut output);
            for message in output {
                send(&message)?;
            }
        }
        Ok(())
    }

    /// Shared CC64-up / CC121 release path. Physically held voices keep their
    /// channels and glides; sustained voices receive explicit mapped NoteOffs.
    fn release_pedal<E>(
        &mut self,
        value: u8,
        reset_controllers: bool,
        now: Duration,
        send: &mut impl FnMut(&MidiMessage) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut released = [None; 15];
        let mut count = 0;
        for voice in self
            .allocator
            .voices()
            .iter()
            .filter(|v| v.state == VoiceState::ReleasedSustained)
        {
            released[count] = Some(*voice);
            count += 1;
        }
        let released = &released[..count];
        for channel in 1..=15 {
            send(&[0xb0 | channel, 64, value])?;
        }
        for voice in released.iter().flatten() {
            send(&[0x80 | voice.output_channel, voice.output_note, 0])?;
        }
        if reset_controllers {
            diagnostic!(
                self,
                info,
                "RESET input=CC121 releasing={} held={}",
                released.len(),
                self.allocator.state_counts().0
            );
            // Recover stray downstream voices on channels that have no held
            // key, even if local bookkeeping is empty. Held channels must not
            // receive All Sound Off during a controller-only reset.
            for channel in 1..=15 {
                if !self
                    .allocator
                    .voices()
                    .iter()
                    .any(|voice| voice.output_channel == channel && voice.state == VoiceState::Held)
                {
                    send(&[0xb0 | channel, 120, 0])?;
                }
            }
            for channel in 1..=15 {
                send(&[0xb0 | channel, 121, 0])?;
            }
            // CC121 resets pitch bend in the receiver. Restore the configured
            // sensitivity and current emitted bend for held voices, including
            // static ones whose glide would otherwise emit nothing afterward.
            for voice in self
                .allocator
                .voices()
                .iter()
                .filter(|v| v.state == VoiceState::Held)
            {
                for message in self.bend_range.rpn(voice.output_channel) {
                    send(&message)?;
                }
                if let Some(bend) = self.glide.last_emitted_bend(voice.id) {
                    send(&encode_pitch_bend(voice.output_channel, bend))?;
                }
            }
        }
        for voice in released.iter().flatten() {
            send(&encode_pitch_bend(voice.output_channel, CENTER))?;
        }
        if self.sustain_down {
            diagnostic!(self, info, "PEDAL UP");
        }
        self.sustain_down = false;
        self.allocator.remove_sustained();
        for voice in released.iter().flatten() {
            self.forget_pending(voice.id);
            self.glide.remove(voice.id);
            diagnostic!(
                self,
                info,
                "VOICE RELEASE {} voice={} ch={} output_note={}",
                note_name(voice.input_note),
                voice.id,
                voice.output_channel + 1,
                voice.output_note
            );
        }
        if !released.is_empty() {
            let mut output = EventBuffer::new();
            self.retune(now, &mut output);
            for message in output {
                send(&message)?;
            }
        }
        Ok(())
    }

    fn retune(&mut self, now: Duration, output: &mut EventBuffer) {
        let (held, sustained) = self.allocator.state_counts();
        diagnostic!(self, info, "VOICES held={held} sustained={sustained}");
        // One immutable timestamp is shared by tuning and output retargeting.
        // This closes the feedback loop around the actually audible position.
        let mut notes = [ActiveNote {
            id: 0,
            input_note: 0,
            current_offset_cents: None,
        }; 15];
        let len = self.snapshot(now, &mut notes);
        self.tuning
            .tune_into(&notes[..len], &mut self.tuned_scratch);
        for tuned in &self.tuned_scratch {
            if self.pending_attacks.contains(&tuned.id) {
                continue;
            }
            if let Some(voice) = self.allocator.voice_mut(tuned.id) {
                voice.cents_offset = tuned.cents_offset;
                let output_cents = tuned.cents_offset
                    + 100.0 * (f64::from(voice.input_note) - f64::from(voice.output_note));
                if output_cents.abs() > self.bend_range.semitones() * 100.0 + 1e-6 {
                    diagnostic!(
                        self,
                        warn,
                        "Pitch bend clipped for {} voice={} ch={}: {output_cents:+.3}c exceeds ±{} semitones",
                        note_name(voice.input_note),
                        voice.id,
                        voice.output_channel + 1,
                        self.bend_range.semitones()
                    );
                }
                let bend = cents_to_pitch_bend(output_cents, self.bend_range.semitones());
                match self.glide.retarget_from(
                    tuned.id,
                    self.glide.current_offset_at(tuned.id, now),
                    output_cents,
                    now,
                ) {
                    TargetUpdate::Immediate(value) => {
                        output.push(encode_pitch_bend(voice.output_channel, value));
                    }
                    TargetUpdate::Scheduled {
                        from_cents,
                        to_cents,
                    } => {
                        diagnostic!(
                            self,
                            info,
                            "RETUNE   {:<4} voice={} {from_cents:+.3}c -> {to_cents:+.3}c glide={}ms",
                            note_name(voice.input_note),
                            voice.id,
                            self.glide.config().duration_ms
                        );
                    }
                    TargetUpdate::Unchanged => {}
                }
                diagnostic!(
                    self,
                    info,
                    "TUNE     {:<4} {:<7} {:+.3}c  bend={bend}",
                    note_name(voice.input_note),
                    self.tuning.label(tuned),
                    tuned.cents_offset
                );
            }
        }
    }

    pub fn next_glide_deadline(&self) -> Option<Duration> {
        self.glide.next_deadline()
    }

    pub fn retune_to(&mut self, now: Duration, mut send: impl FnMut(MidiMessage)) {
        let mut output = EventBuffer::new();
        self.retune(now, &mut output);
        for message in output {
            send(message);
        }
    }

    /// Send due bend updates only; never calls the tuning optimizer.
    pub fn tick_at(&mut self, now: Duration) -> Vec<MidiMessage> {
        let mut output = Vec::new();
        self.tick_at_to(now, |message| output.push(message));
        output
    }

    pub fn tick_at_to(&mut self, now: Duration, mut output: impl FnMut(MidiMessage)) {
        let voices = self.allocator.voices();
        let diagnostics = self.diagnostics;
        self.glide.tick_to(now, |update| {
            if let Some(voice) = voices.iter().find(|voice| voice.id == update.voice_id) {
                output(encode_pitch_bend(voice.output_channel, update.bend));
                if diagnostics {
                    log::debug!(
                        "GLIDE    voice={} ch={} bend={}",
                        voice.id,
                        voice.output_channel + 1,
                        update.bend
                    );
                }
            }
        });
    }

    pub fn shutdown(&mut self) -> Vec<MidiMessage> {
        let mut output = Vec::with_capacity(78);
        self.panic_output(PanicKind::Shutdown, |message| {
            output.push(*message);
            Ok::<_, Infallible>(())
        })
        .unwrap();
        output
    }

    /// Common cleanup for explicit panics and automatic plugin resets.
    /// Try every termination message even if a send fails. Keep voice/FIFO
    /// bookkeeping on failure so another attempt can still address each voice.
    pub fn panic_output<E>(
        &mut self,
        kind: PanicKind,
        mut send: impl FnMut(&MidiMessage) -> Result<(), E>,
    ) -> Result<(), E> {
        let (held, sustained) = self.allocator.state_counts();
        let input = match kind {
            PanicKind::AllSoundOff => "CC120",
            PanicKind::AllNotesOff => "CC123",
            PanicKind::Shutdown => "shutdown",
            PanicKind::HostReset => "host-reset",
        };
        diagnostic!(
            self,
            info,
            "PANIC input={input} active={} held={held} sustained={sustained}",
            held + sustained
        );
        let mut error = None;
        let mut attempt = |message| {
            if let Err(failure) = send(&message) {
                error.get_or_insert(failure);
            }
        };
        if kind == PanicKind::AllSoundOff {
            for channel in 0..=15 {
                attempt([0xb0 | channel, 120, 0]);
            }
        }
        for voice in self.allocator.voices() {
            diagnostic!(
                self,
                info,
                "PANIC NOTE_OFF ch={} note={} voice={}",
                voice.output_channel + 1,
                voice.output_note,
                voice.id
            );
            attempt([0x80 | voice.output_channel, voice.output_note, 0]);
        }
        for channel in 0..=15 {
            // All Notes Off alone does not silence sustain-held notes.
            attempt([0xb0 | channel, 64, 0]);
        }
        let broadcast_kill = kind != PanicKind::HostReset || held + sustained != 0;
        if broadcast_kill {
            for channel in 0..=15 {
                attempt([0xb0 | channel, 123, 0]);
            }
            if kind != PanicKind::AllSoundOff {
                for channel in 0..=15 {
                    attempt([0xb0 | channel, 120, 0]);
                }
            }
        }
        diagnostic!(
            self,
            info,
            "PANIC broadcast CC64=0 ch=1..16; CC123/CC120={broadcast_kill}"
        );
        // Do not sweep pitch after a failed termination. Cancel scheduling, but
        // retain mapped identities and pending releases for fatal cleanup/retry.
        if let Some(error) = error {
            self.glide.clear();
            diagnostic!(
                self,
                warn,
                "PANIC incomplete; retaining {} voices for retry",
                held + sustained
            );
            return Err(error);
        }
        let mut error = None;
        for channel in 1..=15 {
            if let Err(failure) = send(&encode_pitch_bend(channel, CENTER)) {
                error.get_or_insert(failure);
            }
        }
        self.glide.clear();
        if let Some(error) = error {
            return Err(error);
        }
        self.allocator.clear();
        self.pending_attacks.fill(0);
        self.sustain_down = false;
        self.tuning.tune_into(&[], &mut self.tuned_scratch);
        diagnostic!(
            self,
            info,
            "PANIC sent; local voices=0 held=0 sustained=0 (no downstream acknowledgement)"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::{cell::Cell, rc::Rc};

    use super::*;
    use crate::tuning::FiveLimitLowestRoot;
    use crate::tuning::{
        OptimizerConfig,
        profile::{SpectrumPreset, SpectrumProfile},
        scala::{KeyboardMapping, ScalaScale, ScalaTuning},
    };

    fn scala_processor(
        tuning: ScalaTuning,
    ) -> MpeProcessor<crate::tuning::AdaptiveRationalOptimizer> {
        let config = OptimizerConfig::default();
        let profile = SpectrumProfile::builtin(SpectrumPreset::Harmonic, config.roughness).unwrap();
        let optimizer = crate::tuning::AdaptiveRationalOptimizer::with_profile_and_scale(
            config,
            profile,
            Some(tuning),
        )
        .unwrap();
        MpeProcessor::new(optimizer, "2".parse().unwrap())
    }
    use crate::voice::VoiceState;

    fn processor() -> MpeProcessor<FiveLimitLowestRoot> {
        MpeProcessor::new(FiveLimitLowestRoot, "2".parse().unwrap())
    }

    fn remapped_processor() -> MpeProcessor<crate::tuning::AdaptiveRationalOptimizer> {
        // D3/input 50 maps to output 60, and input 51 maps to output 62.
        let scale = ScalaScale::parse("wide\n2\n150.0\n1200.0\n").unwrap();
        scala_processor(ScalaTuning::new(scale, None, 48, 2.0).unwrap())
    }

    fn controller(number: u8) -> MidiEvent {
        MidiEvent::ControlChange {
            channel: 0,
            controller: number,
            value: 0,
        }
    }

    #[test]
    fn empty_host_reset_does_not_send_note_kills_but_resets_pedal_and_bends() {
        let mut processor = processor();
        processor.process(pedal(127));
        let mut messages = Vec::new();
        processor
            .panic_output(PanicKind::HostReset, |message| {
                messages.push(*message);
                Ok::<_, Infallible>(())
            })
            .unwrap();
        assert!(!processor.sustain_down);
        assert!(processor.voices().is_empty());
        assert!(
            !messages
                .iter()
                .any(|m| m[0] & 0xf0 == 0xb0 && matches!(m[1], 120 | 123))
        );
        for channel in 1..=15 {
            assert!(messages.contains(&[0xb0 | channel, 64, 0]));
            assert!(messages.contains(&encode_pitch_bend(channel, CENTER)));
        }
        assert!(processor.next_glide_deadline().is_none());
    }

    #[test]
    fn panics_explicitly_terminate_all_mapped_held_and_sustained_voices() {
        for kind in [
            PanicKind::AllNotesOff,
            PanicKind::AllSoundOff,
            PanicKind::Shutdown,
            PanicKind::HostReset,
        ] {
            let mut processor = remapped_processor();
            processor.process(on(50));
            processor.process(pedal(127));
            processor.process(off(50));
            processor.process(on(50));
            let voices = processor.voices().to_vec();
            assert_eq!(processor.allocator.state_counts(), (1, 1));
            let mut messages = Vec::new();
            processor
                .panic_output(kind, |message| {
                    messages.push(*message);
                    Ok::<_, Infallible>(())
                })
                .unwrap();
            for voice in voices {
                assert_eq!(voice.output_note, 60);
                let off = [0x80 | voice.output_channel, voice.output_note, 0];
                let off_index = messages.iter().position(|message| *message == off).unwrap();
                let center_index = messages
                    .iter()
                    .position(|message| *message == encode_pitch_bend(voice.output_channel, CENTER))
                    .unwrap();
                assert!(off_index < center_index);
                assert!(!messages.contains(&[0x80 | voice.output_channel, 50, 0]));
            }
            if kind == PanicKind::AllSoundOff {
                assert_eq!(messages[0], [0xb0, 120, 0]);
            } else {
                assert_eq!(&messages[..2], &[[0x81, 60, 0], [0x82, 60, 0]]);
            }
            for channel in 1..=15 {
                for cc in [64, 120, 123] {
                    let index = messages
                        .iter()
                        .position(|msg| *msg == [0xb0 | channel, cc, 0])
                        .unwrap();
                    let center = messages
                        .iter()
                        .position(|msg| *msg == encode_pitch_bend(channel, CENTER))
                        .unwrap();
                    assert!(index < center);
                }
            }
            assert!(processor.voices().is_empty());
            assert!(!processor.sustain_down);
            assert!(processor.process(off(50)).is_empty());
            assert!(processor.next_glide_deadline().is_none());
        }
    }

    #[test]
    fn panic_with_empty_bookkeeping_kills_untracked_downstream_voices() {
        for cc in [120, 121, 123] {
            let mut processor = processor();
            // Simulate a desynchronized synth, including one that ignores CC123.
            let mut sounding_channels: std::collections::BTreeSet<_> = (1..=15).collect();
            processor
                .process_at_to(controller(cc), Duration::ZERO, |message| {
                    if message[0] & 0xf0 == 0xb0 && message[1] == 120 && message[2] == 0 {
                        sounding_channels.remove(&(message[0] & 0x0f));
                    }
                    Ok::<_, Infallible>(())
                })
                .unwrap();
            assert!(sounding_channels.is_empty());
            assert!(processor.voices().is_empty());
            assert!(processor.tick_at(Duration::from_secs(1)).is_empty());
        }
    }

    #[test]
    fn pedal_up_and_reset_controllers_release_mapped_sustained_voices_only() {
        for event in [pedal(0), controller(121)] {
            let mut processor = remapped_processor();
            processor.process(on(50));
            processor.process(on(51));
            processor.process(pedal(127));
            processor.process(off(50));
            let held_id = processor.voices()[1].id;
            let messages = processor.process(event);
            assert_eq!(messages[14], [0xbf, 64, 0]);
            assert_eq!(messages[15], [0x81, 60, 0]);
            let centered = messages
                .iter()
                .position(|msg| *msg == encode_pitch_bend(1, CENTER))
                .unwrap();
            assert!(centered > 15);
            assert!(
                !messages
                    .iter()
                    .any(|msg| msg[0] == 0x82 || msg[0] & 0xf0 == 0x90)
            );
            assert_eq!(processor.voices().len(), 1);
            assert_eq!(processor.voices()[0].id, held_id);
            assert_eq!(processor.voices()[0].state, VoiceState::Held);
            assert!(!processor.sustain_down);
            assert_eq!(processor.process(off(51))[0], [0x82, 62, 45]);
        }
    }

    #[test]
    fn reset_controllers_restores_held_bends_without_retriggering_or_restarting_glide() {
        for duration_ms in [0, 35] {
            let mut processor = MpeProcessor::with_glide(
                FiveLimitLowestRoot,
                "2".parse().unwrap(),
                GlideConfig {
                    duration_ms,
                    tick_ms: 5,
                },
            );
            processor.process_at(on(64), ms(0));
            processor.process_at(on(60), ms(1));
            processor.tick_at(ms(11));
            let voice_id = processor.voices()[0].id;
            let expected_bend = processor.glide.last_emitted_bend(voice_id).unwrap();
            assert_ne!(expected_bend, CENTER);
            let deadline = processor.next_glide_deadline();
            let messages = processor.process_at(controller(121), ms(12));
            for channel in 1..=15 {
                assert!(messages.contains(&[0xb0 | channel, 121, 0]));
            }
            assert!(messages.contains(&encode_pitch_bend(1, expected_bend)));
            assert!(
                !messages
                    .iter()
                    .any(|msg| matches!(msg[0] & 0xf0, 0x80 | 0x90))
            );
            assert_eq!(processor.next_glide_deadline(), deadline);
            assert_eq!(processor.voices().len(), 2);
            if duration_ms > 0 {
                assert_eq!(processor.tick_at(ms(100)), [encode_pitch_bend(1, 7631)]);
            }
            assert!(!messages.contains(&[0xb1, 120, 0]));
            assert!(!messages.contains(&[0xb2, 120, 0]));
            assert!(messages.contains(&[0xb3, 120, 0]));
        }
    }

    #[test]
    fn failed_panic_retains_mapped_bookkeeping_and_attempts_all_termination_channels() {
        let mut processor = remapped_processor();
        processor.process(on(50));
        processor.process(on(51));
        let mut attempted = Vec::new();
        let error = processor.process_at_to(controller(123), Duration::ZERO, |message| {
            attempted.push(*message);
            if *message == [0x81, 60, 0] {
                Err("simulated send failure")
            } else {
                Ok(())
            }
        });
        assert!(error.is_err());
        assert_eq!(processor.voices().len(), 2);
        assert!(processor.allocator.next_release(0, 50).is_some());
        assert!(attempted.contains(&[0x82, 62, 0]));
        assert!(attempted.contains(&[0xbf, 120, 0]));
        assert!(!attempted.iter().any(|msg| msg[0] & 0xf0 == 0xe0));
        assert!(processor.next_glide_deadline().is_none());
        let cleanup = processor.shutdown();
        assert!(cleanup.contains(&[0x81, 60, 0]));
        assert!(cleanup.contains(&[0x82, 62, 0]));
        assert!(processor.voices().is_empty());
    }

    #[test]
    fn failed_note_off_and_pedal_release_keep_voice_identity_for_fatal_panic() {
        for sustained in [false, true] {
            let mut processor = remapped_processor();
            processor.process(on(50));
            if sustained {
                processor.process(pedal(127));
                processor.process(off(50));
            }
            let event = if sustained { pedal(0) } else { off(50) };
            assert!(
                processor
                    .process_at_to(event, Duration::ZERO, |message| {
                        if message[0] == 0x81 { Err(()) } else { Ok(()) }
                    })
                    .is_err()
            );
            assert_eq!(processor.voices().len(), 1);
            if !sustained {
                assert!(processor.allocator.next_release(0, 50).is_some());
            }
            assert!(processor.shutdown().contains(&[0x81, 60, 0]));
        }
    }

    #[test]
    fn repeated_loop_panics_clear_all_voices_and_rejected_note_fifo_entries() {
        let mut processor = remapped_processor();
        for cc in [123, 120, 123, 123, 120] {
            processor.process(pedal(127));
            for _ in 0..15 {
                processor.process(on(50));
            }
            assert!(processor.process(on(50)).is_empty()); // rejected 16th NoteOn
            processor.process(off(50)); // one sustained voice + 14 held
            let messages = processor.process(controller(cc));
            for channel in 1..=15 {
                assert!(messages.contains(&[0x80 | channel, 60, 0]));
            }
            assert!(processor.voices().is_empty());
            processor.process(on(50));
            assert_eq!(processor.process(off(50))[0], [0x81, 60, 45]);
            assert!(processor.voices().is_empty());
            assert!(processor.process(off(50)).is_empty());
        }
    }

    #[test]
    fn adaptive_seventh_and_reference_change_update_bends_without_retrigger() {
        let mut processor = MpeProcessor::new(
            crate::tuning::AdaptiveRationalOptimizer::default(),
            "2".parse().unwrap(),
        );
        for note in [60, 64, 67] {
            processor.process(on(note));
        }
        let added = processor.process(on(70));
        assert_eq!(added.iter().filter(|msg| msg[0] & 0xf0 == 0x90).count(), 1);
        let seventh = processor
            .voices()
            .iter()
            .find(|voice| voice.input_note == 70)
            .unwrap();
        assert!(added.contains(&encode_pitch_bend(
            4,
            cents_to_pitch_bend(seventh.cents_offset, 2.0)
        )));
        let removed = processor.process(off(60));
        assert!(!removed.iter().any(|msg| msg[0] & 0xf0 == 0x90));
        assert_eq!(
            removed.iter().filter(|msg| msg[0] & 0xf0 == 0xe0).count(),
            4
        );
        let e = processor
            .voices()
            .iter()
            .find(|voice| voice.input_note == 64)
            .unwrap();
        assert!(removed.contains(&encode_pitch_bend(
            2,
            cents_to_pitch_bend(e.cents_offset, 2.0)
        )));
        assert_eq!(processor.voices()[0].input_note, 64);
        let reference = &processor.voices()[0];
        assert_eq!(reference.input_note, 64);
        assert!(reference.cents_offset.abs() > 1.0);
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

    fn pedal(value: u8) -> MidiEvent {
        MidiEvent::ControlChange {
            channel: 0,
            controller: 64,
            value,
        }
    }

    fn glide_processor() -> MpeProcessor<FiveLimitLowestRoot> {
        MpeProcessor::with_glide(
            FiveLimitLowestRoot,
            "2".parse().unwrap(),
            GlideConfig {
                duration_ms: 20,
                tick_ms: 5,
            },
        )
    }

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn mapped_note_mid_glide_snapshot_is_converted_back_to_input_pitch() {
        struct MappingProbe(Rc<RefCell<Vec<Vec<ActiveNote>>>>);
        impl TuningStrategy for MappingProbe {
            fn output_note(&self, note: u8) -> u8 {
                if note == 61 { 62 } else { note }
            }

            fn tune(&mut self, notes: &[ActiveNote]) -> Vec<crate::tuning::TunedNote> {
                self.0.borrow_mut().push(notes.to_vec());
                notes
                    .iter()
                    .map(|note| crate::tuning::TunedNote {
                        id: note.id,
                        ratio: crate::tuning::Ratio::ONE,
                        cents_offset: if note.input_note == 61 {
                            if notes.len() == 1 { 150.0 } else { 130.0 }
                        } else {
                            0.0
                        },
                    })
                    .collect()
            }
        }
        let observed = Rc::new(RefCell::new(Vec::new()));
        let mut processor = MpeProcessor::with_glide(
            MappingProbe(Rc::clone(&observed)),
            "2".parse().unwrap(),
            GlideConfig {
                duration_ms: 100,
                tick_ms: 5,
            },
        );
        assert_eq!(
            processor.process_at(on(61), ms(0)).last(),
            Some(&[0x91, 62, 100])
        );
        processor.process_at(on(64), ms(10));
        processor.process_at(on(67), ms(35));
        let calls = observed.borrow();
        let input_relative = calls[2]
            .iter()
            .find(|note| note.input_note == 61)
            .unwrap()
            .current_offset_cents
            .unwrap();
        assert!((input_relative - 145.0).abs() < 1e-9);
    }

    #[test]
    fn scala_remaps_note_on_and_all_release_paths_consistently() {
        let scale = ScalaScale::parse("wide\n2\n150.0\n1200.0\n").unwrap();
        let tuning = ScalaTuning::new(scale, None, 60, 2.0).unwrap();
        let mut processor = scala_processor(tuning);
        let messages = processor.process(on(61));
        assert_eq!(messages.last(), Some(&[0x91, 62, 100]));
        assert_eq!(processor.voices()[0].input_note, 61);
        assert_eq!(processor.voices()[0].output_note, 62);
        assert!(messages.contains(&encode_pitch_bend(1, cents_to_pitch_bend(-50.0, 2.0))));
        let off_messages = processor.process(off(61));
        assert!(off_messages.contains(&[0x81, 62, 45]));

        processor.process(on(61));
        processor.process(pedal(127));
        let sustained_off = processor.process(off(61));
        assert!(sustained_off.contains(&[0x81, 62, 45]));
        assert_eq!(processor.voices().len(), 1);
        processor.process(pedal(0));
        assert!(processor.voices().is_empty());
        processor.process(on(61));
        let shutdown = processor.shutdown();
        assert!(shutdown.contains(&[0x81, 62, 0]));
    }

    #[test]
    fn scala_unmapped_keys_do_not_consume_channels_or_release_other_voices() {
        let scale = ScalaScale::parse("gaps\n2\n100.0\n1200.0\n").unwrap();
        let map = KeyboardMapping::parse("2\n60\n70\n60\n60\n261.625565\n2\n0\nx\n").unwrap();
        let tuning = ScalaTuning::new(scale, Some(map), 60, 2.0).unwrap();
        let mut processor = scala_processor(tuning);
        assert!(processor.process(on(61)).is_empty());
        assert!(processor.voices().is_empty());
        assert_eq!(processor.process(on(62)).last(), Some(&[0x91, 72, 100]));
        assert!(processor.process(off(61)).is_empty());
        assert_eq!(processor.voices().len(), 1);
        assert!(processor.process(off(62)).contains(&[0x81, 72, 45]));
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

    #[test]
    fn note_off_under_pedal_keeps_channel_and_bend_until_pedal_up() {
        let mut processor = processor();
        processor.process(on(60));
        processor.process(on(64));
        processor.process(pedal(127));
        let before = processor.voices()[1].cents_offset;
        let released = processor.process(off(64));
        assert_eq!(released, vec![[0x82, 64, 45]]);
        assert_eq!(processor.voices()[1].state, VoiceState::ReleasedSustained);
        assert_eq!(processor.voices()[1].cents_offset, before);
        let added = processor.process(on(67));
        assert_eq!(added.last(), Some(&[0x93, 67, 100]));
        assert_eq!(processor.voices().len(), 3);
        let up = processor.process(pedal(0));
        assert_eq!(
            &up[..15],
            &(1..=15).map(|ch| [0xb0 | ch, 64, 0]).collect::<Vec<_>>()
        );
        assert_eq!(up[15], [0x82, 64, 0]);
        assert_eq!(up[16], encode_pitch_bend(2, CENTER));
        assert!(!up.iter().any(|msg| msg[0] & 0xf0 == 0x90));
        assert_eq!(processor.voices().len(), 2);
        assert!(
            processor
                .voices()
                .iter()
                .all(|v| v.state == VoiceState::Held)
        );
    }

    #[test]
    fn sustained_voices_participate_in_tuning_and_can_retune() {
        let mut processor = processor();
        processor.process(on(64));
        processor.process(pedal(127));
        processor.process(off(64));
        assert_eq!(processor.voices().len(), 1);
        assert_eq!(processor.voices()[0].state, VoiceState::ReleasedSustained);
        let added = processor.process(on(60));
        assert!(added.iter().any(|msg| msg[0] == 0xe1));
        assert!((processor.voices()[0].cents_offset + 13.686286).abs() < 1e-4);
        assert_eq!(processor.voices()[0].state, VoiceState::ReleasedSustained);
        assert_eq!(added.last(), Some(&[0x92, 60, 100]));
    }

    #[test]
    fn restrike_and_repeated_notes_under_pedal_remain_independent() {
        let mut processor = processor();
        processor.process(pedal(127));
        processor.process(on(60));
        processor.process(off(60));
        processor.process(on(60));
        assert_eq!(processor.voices().len(), 2);
        assert_eq!(processor.voices()[0].state, VoiceState::ReleasedSustained);
        assert_eq!(processor.voices()[1].state, VoiceState::Held);
        assert_ne!(processor.voices()[0].id, processor.voices()[1].id);
        assert_ne!(
            processor.voices()[0].output_channel,
            processor.voices()[1].output_channel
        );
        processor.process(off(60));
        assert_eq!(
            processor
                .voices()
                .iter()
                .filter(|v| v.state == VoiceState::ReleasedSustained)
                .count(),
            2
        );
        let up = processor.process(pedal(0));
        assert_eq!(up.len(), 19);
        assert_eq!(up[15], [0x81, 60, 0]);
        assert_eq!(up[16], [0x82, 60, 0]);
        assert_eq!(up[17], encode_pitch_bend(1, CENTER));
        assert_eq!(up[18], encode_pitch_bend(2, CENTER));
        assert!(processor.voices().is_empty());
    }

    #[test]
    fn fifteen_sustained_voices_block_new_note_until_pedal_up() {
        let mut processor = processor();
        processor.process(pedal(127));
        for note in 60..75 {
            processor.process(on(note));
            assert_eq!(
                processor.process(off(note)),
                vec![[0x80 | (note - 59), note, 45]]
            );
        }
        assert_eq!(processor.voices().len(), 15);
        assert!(processor.process(on(75)).is_empty());
        assert!(processor.process(off(75)).is_empty());
        let up = processor.process(pedal(0));
        assert_eq!(up.len(), 45);
        assert!(processor.voices().is_empty());
        assert_eq!(processor.process(on(76)).last(), Some(&[0x91, 76, 100]));
    }

    #[test]
    fn repeated_pedal_messages_are_harmless_and_panic_clears_sustain() {
        for controller in [120, 123] {
            let mut processor = processor();
            processor.process(pedal(64));
            processor.process(pedal(127));
            processor.process(on(60));
            processor.process(off(60));
            assert_eq!(processor.voices().len(), 1);
            let panic = processor.process(MidiEvent::ControlChange {
                channel: 0,
                controller,
                value: 0,
            });
            let pedal_up = panic.iter().position(|msg| *msg == [0xb1, 64, 0]).unwrap();
            let all_off = panic.iter().position(|msg| *msg == [0xb1, 123, 0]).unwrap();
            let centered = panic
                .iter()
                .position(|msg| *msg == encode_pitch_bend(1, CENTER))
                .unwrap();
            assert!(pedal_up < all_off && all_off < centered);
            assert!(processor.voices().is_empty());
            assert_eq!(processor.process(pedal(0)).len(), 15);
            processor.process(on(61));
            assert_eq!(processor.process(off(61))[1], encode_pitch_bend(1, CENTER));
        }
    }

    #[test]
    fn sustained_reference_preserves_floating_anchor_continuity() {
        let mut processor = MpeProcessor::new(
            crate::tuning::AdaptiveRationalOptimizer::default(),
            "2".parse().unwrap(),
        );
        for note in [60, 64, 67] {
            processor.process(on(note));
        }
        let before: Vec<_> = processor.voices().iter().map(|v| v.cents_offset).collect();
        processor.process(pedal(127));
        let off_output = processor.process(off(60));
        assert_eq!(off_output, vec![[0x81, 60, 45]]);
        assert_eq!(processor.voices()[0].state, VoiceState::ReleasedSustained);
        for (voice, offset) in processor.voices().iter().zip(&before) {
            assert!((voice.cents_offset - offset).abs() < 1e-9);
        }
        let up = processor.process(pedal(0));
        assert_eq!(up[15], [0x81, 60, 0]);
        assert_eq!(up[16], encode_pitch_bend(1, CENTER));
        assert_eq!(processor.voices().len(), 2);
        assert!((processor.voices()[0].cents_offset - before[1]).abs() < 2.0);
        assert!((processor.voices()[1].cents_offset - before[2]).abs() < 2.0);
    }

    #[test]
    fn shutdown_with_sustained_voices_releases_pedal_and_resets_tuning_history() {
        let mut processor = MpeProcessor::new(
            crate::tuning::AdaptiveRationalOptimizer::default(),
            "2".parse().unwrap(),
        );
        for note in [60, 64, 67] {
            processor.process(on(note));
        }
        processor.process(pedal(127));
        processor.process(off(60));
        processor.process(off(64));
        assert_eq!(processor.voices().len(), 3);
        let cleanup = processor.shutdown();
        let last_pedal_up = cleanup
            .iter()
            .rposition(|msg| msg[1] == 64 && msg[2] == 0)
            .unwrap();
        let first_all_off = cleanup.iter().position(|msg| msg[1] == 123).unwrap();
        let first_center = cleanup
            .iter()
            .position(|msg| *msg == encode_pitch_bend(1, CENTER))
            .unwrap();
        assert!(last_pedal_up < first_all_off && first_all_off < first_center);
        assert!(processor.voices().is_empty());
        processor.process(on(69));
        assert!(processor.voices()[0].cents_offset.abs() < 1e-9);
        assert_eq!(processor.process(off(69))[1], encode_pitch_bend(1, CENTER));
    }

    #[test]
    fn sustain_event_sequence_is_deterministic() {
        let events = [
            on(60),
            pedal(127),
            off(60),
            on(64),
            off(64),
            pedal(0),
            on(67),
        ];
        let mut first = processor();
        let mut second = processor();
        for event in events {
            assert_eq!(first.process(event), second.process(event));
        }
    }

    #[test]
    fn new_note_uses_target_bend_before_attack_while_existing_voice_glides() {
        let mut processor = glide_processor();
        processor.process_at(on(64), ms(0));
        let added = processor.process_at(on(60), ms(10));
        assert_eq!(added.last(), Some(&[0x92, 60, 100]));
        assert_eq!(added[added.len() - 2], encode_pitch_bend(2, CENTER));
        assert!(!added.iter().any(|msg| msg[0] == 0xe1));
        assert_eq!(
            processor.voices()[0].cents_offset,
            1200.0 * (5.0_f64 / 4.0).log2() - 400.0
        );
        let middle = processor.tick_at(ms(20));
        assert_eq!(middle.len(), 1);
        assert_eq!(middle[0][0], 0xe1);
        assert_ne!(middle[0], encode_pitch_bend(1, CENTER));
        assert_ne!(middle[0], encode_pitch_bend(1, 7631));
        let final_update = processor.tick_at(ms(30));
        assert_eq!(final_update, vec![encode_pitch_bend(1, 7631)]);
        assert_eq!(processor.next_glide_deadline(), None);
        assert!(
            middle
                .iter()
                .chain(&final_update)
                .all(|msg| msg[0] & 0xf0 == 0xe0)
        );
    }

    #[test]
    fn sustained_voice_glides_and_pedal_up_cancels_it_before_centering() {
        let mut processor = glide_processor();
        processor.process_at(on(64), ms(0));
        processor.process_at(pedal(127), ms(1));
        assert_eq!(processor.process_at(off(64), ms(2)), vec![[0x81, 64, 45]]);
        processor.process_at(on(60), ms(3));
        assert_eq!(processor.voices()[0].state, VoiceState::ReleasedSustained);
        assert_eq!(processor.tick_at(ms(8))[0][0], 0xe1);
        let up = processor.process_at(pedal(0), ms(9));
        assert_eq!(up[0], [0xb1, 64, 0]);
        assert_eq!(up[15], [0x81, 64, 0]);
        assert_eq!(up[16], encode_pitch_bend(1, CENTER));
        assert!(processor.tick_at(ms(100)).iter().all(|msg| msg[0] != 0xe1));
        assert_eq!(processor.next_glide_deadline(), None);
    }

    #[test]
    fn note_off_cancels_glide_and_channel_reuse_has_no_stale_bend() {
        let mut processor = glide_processor();
        processor.process_at(on(64), ms(0));
        processor.process_at(on(60), ms(1));
        assert!(processor.next_glide_deadline().is_some());
        let off_output = processor.process_at(off(64), ms(2));
        assert_eq!(off_output[0], [0x81, 64, 45]);
        assert_eq!(off_output[1], encode_pitch_bend(1, CENTER));
        let reused = processor.process_at(on(67), ms(3));
        assert_eq!(reused.last(), Some(&[0x91, 67, 100]));
        assert_eq!(reused[reused.len() - 2], encode_pitch_bend(1, 8272));
        assert!(processor.tick_at(ms(100)).iter().all(|msg| msg[0] != 0xe1));
        assert_eq!(processor.next_glide_deadline(), None);
    }

    #[test]
    fn repeated_note_voices_glide_by_id_and_finish_together() {
        let mut processor = glide_processor();
        processor.process_at(on(64), ms(0));
        processor.process_at(on(64), ms(1));
        let ids: Vec<_> = processor.voices().iter().map(|voice| voice.id).collect();
        assert_ne!(ids[0], ids[1]);
        processor.process_at(on(60), ms(2));
        let middle = processor.tick_at(ms(12));
        assert_eq!(middle.len(), 2);
        assert_eq!(
            middle.iter().map(|msg| msg[0]).collect::<Vec<_>>(),
            vec![0xe1, 0xe2]
        );
        let done = processor.tick_at(ms(22));
        assert_eq!(
            done,
            vec![encode_pitch_bend(1, 7631), encode_pitch_bend(2, 7631)]
        );
    }

    #[test]
    fn panic_and_shutdown_cancel_all_pending_glides() {
        for panic in [None, Some(120), Some(123)] {
            let mut processor = glide_processor();
            processor.process_at(on(64), ms(0));
            processor.process_at(on(60), ms(1));
            assert!(processor.next_glide_deadline().is_some());
            if let Some(controller) = panic {
                processor.process_at(
                    MidiEvent::ControlChange {
                        channel: 0,
                        controller,
                        value: 0,
                    },
                    ms(2),
                );
            } else {
                processor.shutdown();
            }
            assert_eq!(processor.next_glide_deadline(), None);
            assert!(processor.tick_at(ms(100)).is_empty());
            assert!(processor.voices().is_empty());
        }
    }

    #[test]
    fn glide_output_uses_actual_mid_glide_position_for_optimizer() {
        let mut instant = MpeProcessor::new(
            crate::tuning::AdaptiveRationalOptimizer::default(),
            "2".parse().unwrap(),
        );
        let mut smooth = MpeProcessor::with_glide(
            crate::tuning::AdaptiveRationalOptimizer::default(),
            "2".parse().unwrap(),
            GlideConfig::default(),
        );
        let events = [
            on(60),
            on(64),
            on(67),
            pedal(127),
            off(60),
            on(70),
            pedal(0),
        ];
        for (index, event) in events.into_iter().enumerate() {
            instant.process(event);
            smooth.process_at(event, ms(index as u64 * 3));
            let a = instant
                .voices()
                .iter()
                .map(|v| (v.id, v.cents_offset))
                .collect::<Vec<_>>();
            let b = smooth
                .voices()
                .iter()
                .map(|v| (v.id, v.cents_offset))
                .collect::<Vec<_>>();
            if index == 0 {
                assert_eq!(a, b);
            }
        }
    }

    #[test]
    fn mock_timed_glide_is_deterministic_without_sleeping() {
        let mut first = glide_processor();
        let mut second = glide_processor();
        for processor in [&mut first, &mut second] {
            processor.process_at(on(64), ms(0));
        }
        assert_eq!(
            first.process_at(on(60), ms(4)),
            second.process_at(on(60), ms(4))
        );
        for time in [5, 9, 13, 19, 24, 50] {
            assert_eq!(first.tick_at(ms(time)), second.tick_at(ms(time)));
        }
    }

    #[test]
    fn glide_ticks_never_call_optimizer() {
        struct Counted {
            calls: Rc<Cell<usize>>,
        }

        impl TuningStrategy for Counted {
            fn tune(&mut self, notes: &[ActiveNote]) -> Vec<crate::tuning::TunedNote> {
                self.calls.set(self.calls.get() + 1);
                FiveLimitLowestRoot.tune(notes)
            }
        }

        let calls = Rc::new(Cell::new(0));
        let mut processor = MpeProcessor::with_glide(
            Counted {
                calls: Rc::clone(&calls),
            },
            "2".parse().unwrap(),
            GlideConfig {
                duration_ms: 20,
                tick_ms: 5,
            },
        );
        processor.process_at(on(64), ms(0));
        processor.process_at(on(60), ms(1));
        assert_eq!(calls.get(), 2);
        for time in 2..=30 {
            processor.tick_at(ms(time));
        }
        assert_eq!(calls.get(), 2);
    }
}

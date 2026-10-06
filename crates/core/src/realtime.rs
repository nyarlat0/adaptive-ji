//! Allocation-free temporal/MPE boundary. No optimizer search, IO, logs or locks.
use crate::{
    event::MidiEvent,
    mpe::{MidiMessage, MpeProcessor, PanicKind},
    settings::{AttackMode, BasePitch, PreparedSettings},
    tuning::{ActiveNote, Ratio, TunedNote, TuningStrategy},
    voice::{Voice, VoiceState},
};
use std::{convert::Infallible, time::Duration};

pub const EMPTY_NOTE: ActiveNote = ActiveNote {
    id: 0,
    input_note: 0,
    current_offset_cents: None,
};
pub const EMPTY_TARGET: TunedNote = TunedNote {
    id: 0,
    ratio: Ratio::ONE,
    cents_offset: 0.0,
};

/// Copy-only telemetry, formatted exclusively on the GUI thread. Cents are
/// relative to the input MIDI note's 12-TET pitch, including Scala base tuning.
#[derive(Clone, Copy, Debug)]
pub struct LiveVoice {
    pub id: u64,
    pub input_note: u8,
    pub output_note: u8,
    pub output_channel: u8,
    pub state: VoiceState,
    pub pending: bool,
    pub current_cents: Option<f64>,
    pub target: Option<TunedNote>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LiveSnapshot {
    pub voices: [Option<LiveVoice>; 15],
    pub reference: Option<(u64, u8)>,
    pub optimizing: bool,
    pub attack_mode: AttackMode,
}

#[derive(Clone, Copy, Debug)]
pub struct RetuneRequest {
    pub generation: u64,
    pub epoch: u64,
    pub sample: u64,
    pub notes: [ActiveNote; 15],
    pub len: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct RetuneResult {
    pub generation: u64,
    pub epoch: u64,
    pub targets: [TunedNote; 15],
    pub len: usize,
}

impl RetuneResult {
    pub fn from_targets(request: RetuneRequest, targets: &[TunedNote]) -> Self {
        let mut result = Self {
            generation: request.generation,
            epoch: request.epoch,
            targets: [EMPTY_TARGET; 15],
            len: targets.len(),
        };
        result.targets[..targets.len()].copy_from_slice(targets);
        result
    }
}

/// Cheap initial path: keep existing targets, attack new notes on their prepared
/// Scala/12-TET base. A future local solver can replace this without touching IO.
struct ImmediateTuning {
    base: [Option<BasePitch>; 128],
    targets: [TunedNote; 15],
    len: usize,
}

impl TuningStrategy for ImmediateTuning {
    fn tune(&mut self, notes: &[ActiveNote]) -> Vec<TunedNote> {
        let mut output = Vec::with_capacity(15);
        self.tune_into(notes, &mut output);
        output
    }
    fn tune_into(&mut self, notes: &[ActiveNote], output: &mut Vec<TunedNote>) {
        output.clear();
        for note in notes {
            let tuned = self.targets[..self.len]
                .iter()
                .find(|target| target.id == note.id)
                .copied()
                .unwrap_or(TunedNote {
                    id: note.id,
                    ratio: Ratio::ONE,
                    cents_offset: self.base[note.input_note as usize]
                        .map_or(0.0, |pitch| pitch.offset),
                });
            output.push(tuned);
        }
        self.len = output.len();
        self.targets[..self.len].copy_from_slice(output);
    }
    fn accepts(&self, note: u8) -> bool {
        self.base[note as usize].is_some()
    }
    fn output_note(&self, note: u8) -> u8 {
        self.base[note as usize].map_or(note, |pitch| pitch.output_note)
    }
}

pub struct RealtimeCore {
    processor: MpeProcessor<ImmediateTuning>,
    generation: u64,
    epoch: u64,
    accepted: u64,
    before: [u64; 15],
    dirty: bool,
    last_reset: Option<u8>,
    reset_on_empty: bool,
    solution: [TunedNote; 15],
    solution_len: usize,
    solution_reference: Option<(u64, u8)>,
    attack_mode: AttackMode,
}

impl RealtimeCore {
    /// Non-realtime construction; does not own or execute the heavy optimizer.
    pub fn new(
        prepared: &PreparedSettings,
        sample_rate: f64,
        reset_on_empty: bool,
    ) -> Result<Self, String> {
        if !sample_rate.is_finite() || !(1.0..=10_000_000.0).contains(&sample_rate) {
            return Err("unsupported sample rate".into());
        }
        let strategy = ImmediateTuning {
            base: prepared.base,
            targets: [EMPTY_TARGET; 15],
            len: 0,
        };
        let mut processor = MpeProcessor::with_glide(strategy, prepared.bend_range, prepared.glide);
        processor.set_diagnostics(false);
        processor.use_sample_clock(sample_rate);
        processor.defer_attacks(prepared.attack_mode == AttackMode::Tuned);
        Ok(Self {
            processor,
            generation: 0,
            epoch: 0,
            accepted: 0,
            before: [0; 15],
            dirty: false,
            last_reset: None,
            reset_on_empty,
            solution: [EMPTY_TARGET; 15],
            solution_len: 0,
            solution_reference: None,
            attack_mode: prepared.attack_mode,
        })
    }
    pub fn voices(&self) -> &[Voice] {
        self.processor.voices()
    }
    pub fn live_snapshot(&self, sample: u64) -> LiveSnapshot {
        let mut snapshot = LiveSnapshot {
            reference: if self.voices().is_empty() {
                None
            } else {
                self.solution_reference
            },
            optimizing: !self.voices().is_empty() && self.accepted != self.generation,
            attack_mode: self.attack_mode,
            ..LiveSnapshot::default()
        };
        let mut notes = [EMPTY_NOTE; 15];
        self.processor
            .snapshot(Duration::from_nanos(sample), &mut notes);
        for ((slot, voice), note) in snapshot.voices.iter_mut().zip(self.voices()).zip(notes) {
            *slot = Some(LiveVoice {
                id: voice.id,
                input_note: voice.input_note,
                output_note: voice.output_note,
                output_channel: voice.output_channel,
                state: voice.state,
                pending: self.processor.is_pending(voice.id),
                current_cents: note.current_offset_cents,
                target: self.solution[..self.solution_len]
                    .iter()
                    .find(|t| t.id == voice.id)
                    .copied(),
            });
        }
        snapshot
    }
    /// Precompute during non-realtime initialization.
    pub fn initialization_messages(&self) -> Vec<MidiMessage> {
        self.processor.initialize()
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn accepted_generation(&self) -> u64 {
        self.accepted
    }
    pub fn begin_batch(&mut self) {
        self.before.fill(0);
        for (slot, voice) in self.before.iter_mut().zip(self.processor.voices()) {
            *slot = voice.id;
        }
        self.dirty = false;
        self.last_reset = None;
    }
    pub fn event(&mut self, sample: u64, event: MidiEvent, mut emit: impl FnMut(u64, MidiMessage)) {
        let valid = match event {
            MidiEvent::NoteOn {
                channel,
                note,
                velocity,
            }
            | MidiEvent::NoteOff {
                channel,
                note,
                velocity,
            } => channel < 16 && note < 128 && velocity < 128,
            MidiEvent::ControlChange {
                channel,
                controller,
                value,
            } => channel < 16 && controller < 128 && value < 128,
        };
        if !valid {
            return;
        }
        // DAWs commonly send one global channel-mode reset on all sixteen
        // input channels. It already broadcasts to the complete output zone.
        // Coalesce adjacent identical resets only within this sample batch;
        // never cross notes, controller changes or sample boundaries.
        let reset = match event {
            MidiEvent::ControlChange {
                controller: c @ (120 | 121 | 123),
                ..
            } => Some(c),
            _ => None,
        };
        if reset.is_some() && reset == self.last_reset {
            return;
        }
        self.last_reset = reset;
        if matches!(
            event,
            MidiEvent::ControlChange {
                controller: 120 | 123,
                ..
            }
        ) {
            self.epoch = self.epoch.wrapping_add(1);
        }
        self.dirty |= !matches!(event, MidiEvent::ControlChange { controller, .. } if !matches!(controller, 64 | 120 | 121 | 123));
        self.processor
            .process_at_to(event, Duration::from_nanos(sample), |message| {
                emit(sample, *message);
                Ok::<_, Infallible>(())
            })
            .unwrap();
    }
    /// Exactly once after all events at this sample offset, in original order.
    pub fn finish_batch(&mut self, sample: u64) -> Option<RetuneRequest> {
        if !self.dirty {
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        if self.reset_on_empty && self.voices().is_empty() && self.before[0] != 0 {
            self.epoch = self.epoch.wrapping_add(1);
        }
        let mut notes = [EMPTY_NOTE; 15];
        let len = self
            .processor
            .snapshot(Duration::from_nanos(sample), &mut notes);
        for note in &mut notes[..len] {
            if !self.before.contains(&note.id) {
                note.current_offset_cents = None;
            }
        }
        self.dirty = false;
        Some(RetuneRequest {
            generation: self.generation,
            epoch: self.epoch,
            sample,
            notes,
            len,
        })
    }
    pub fn apply_result(
        &mut self,
        sample: u64,
        result: RetuneResult,
        mut emit: impl FnMut(u64, MidiMessage),
    ) -> bool {
        if result.generation != self.generation
            || result.epoch != self.epoch
            || result.len != self.voices().len()
            || result.len > 15
        {
            return false;
        }
        for (index, target) in result.targets[..result.len].iter().enumerate() {
            if !target.cents_offset.is_finite()
                || !self.voices().iter().any(|v| v.id == target.id)
                || result.targets[..index].iter().any(|t| t.id == target.id)
            {
                return false;
            }
        }
        let strategy = self.processor.strategy_mut();
        strategy.targets = result.targets;
        strategy.len = result.len;
        self.processor
            .start_pending_to(Duration::from_nanos(sample), |m| emit(sample, m));
        self.solution = result.targets;
        self.solution_len = result.len;
        self.solution_reference = self.voices().first().map(|v| (v.id, v.input_note));
        self.accepted = result.generation;
        true
    }
    /// Emit all deadlines strictly before the boundary; same-sample musical
    /// events are handled first, then tick_at_boundary().
    pub fn advance_before(&mut self, boundary: u64, mut emit: impl FnMut(u64, MidiMessage)) {
        while let Some(deadline) = self.processor.next_glide_deadline() {
            let sample = deadline.as_nanos() as u64;
            if sample >= boundary {
                break;
            }
            self.processor.tick_at_to(deadline, |m| emit(sample, m));
        }
    }
    pub fn tick_at_boundary(&mut self, sample: u64, mut emit: impl FnMut(u64, MidiMessage)) {
        self.processor
            .tick_at_to(Duration::from_nanos(sample), |m| emit(sample, m));
    }
    pub fn reset(&mut self, sample: u64, emit: impl FnMut(u64, MidiMessage)) -> RetuneRequest {
        self.reset_with_kind(sample, PanicKind::Shutdown, emit)
    }
    pub fn reset_for_host(
        &mut self,
        sample: u64,
        emit: impl FnMut(u64, MidiMessage),
    ) -> RetuneRequest {
        self.reset_with_kind(sample, PanicKind::HostReset, emit)
    }
    fn reset_with_kind(
        &mut self,
        sample: u64,
        kind: PanicKind,
        mut emit: impl FnMut(u64, MidiMessage),
    ) -> RetuneRequest {
        self.processor
            .panic_output(kind, |message| {
                emit(sample, *message);
                Ok::<_, Infallible>(())
            })
            .unwrap();
        self.generation = self.generation.wrapping_add(1);
        self.epoch = self.epoch.wrapping_add(1);
        self.dirty = false;
        self.solution_len = 0;
        self.solution_reference = None;
        RetuneRequest {
            generation: self.generation,
            epoch: self.epoch,
            sample,
            notes: [EMPTY_NOTE; 15],
            len: 0,
        }
    }
}

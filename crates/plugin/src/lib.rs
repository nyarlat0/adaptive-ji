//! CLAP/VST3 frontend; the engine has no dependency on this crate.
mod editor;
#[cfg(test)]
mod tests;
use adaptive_ji_core::{event::MidiEvent, realtime::LiveSnapshot, settings::EngineSettings};
use adaptive_ji_runtime::session::Session;
use crossbeam_queue::ArrayQueue;
use logic_nih_plug::prelude::*;
use logic_nih_plug_egui::EguiState;
use std::{
    cell::RefCell,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

#[derive(Params)]
struct Parameters {
    #[persist = "engine"]
    settings: Arc<Mutex<EngineSettings>>,
    #[persist = "editor"]
    editor: Arc<EguiState>,
    // Non-automatable configuration revision informs the host about GUI edits.
    #[id = "configuration-revision"]
    revision: IntParam,
}
impl Default for Parameters {
    fn default() -> Self {
        Self {
            settings: Arc::new(Mutex::new(EngineSettings::default())),
            editor: EguiState::from_size(1020, 900),
            revision: IntParam::new(
                "Configuration revision",
                0,
                IntRange::Linear { min: 0, max: 1023 },
            )
            .non_automatable(),
        }
    }
}
struct PreparedSession {
    rate: u32,
    session: Session,
}
struct Control {
    ready: ArrayQueue<Box<PreparedSession>>,
    retired: ArrayQueue<Box<PreparedSession>>,
    busy: AtomicBool,
    panic: AtomicBool,
    sample_rate: AtomicU32,
    message: Mutex<String>,         // GUI/background only, never process()
    live: ArrayQueue<LiveSnapshot>, // fixed-size copy packets, no GUI locks on RT
}
impl Default for Control {
    fn default() -> Self {
        Self {
            ready: ArrayQueue::new(1),
            retired: ArrayQueue::new(2),
            busy: AtomicBool::new(false),
            panic: AtomicBool::new(false),
            sample_rate: AtomicU32::new(48000.0f32.to_bits()),
            message: Mutex::new(String::new()),
            live: ArrayQueue::new(2),
        }
    }
}

#[derive(Default)]
pub struct AdaptiveJiPlugin {
    params: Arc<Parameters>,
    control: Arc<Control>,
    active: Option<Box<PreparedSession>>,
    cursor: u64,
    expected_position: Option<i64>,
    playing: Option<bool>,
    reset_pending: bool,
    offline: bool,
}

fn input(event: NoteEvent<()>) -> Option<(u32, MidiEvent)> {
    let byte = |v: f32| (v * 127.0).round().clamp(0.0, 127.0) as u8;
    match event {
        NoteEvent::NoteOn {
            timing,
            channel,
            note,
            velocity,
            ..
        } => Some((
            timing,
            MidiEvent::NoteOn {
                channel,
                note,
                velocity: byte(velocity),
            },
        )),
        NoteEvent::NoteOff {
            timing,
            channel,
            note,
            velocity,
            ..
        } => Some((
            timing,
            MidiEvent::NoteOff {
                channel,
                note,
                velocity: byte(velocity),
            },
        )),
        NoteEvent::MidiCC {
            timing,
            channel,
            cc,
            value,
        } => Some((
            timing,
            MidiEvent::ControlChange {
                channel,
                controller: cc,
                value: byte(value),
            },
        )),
        NoteEvent::Choke { timing, .. } => Some((
            timing,
            MidiEvent::ControlChange {
                channel: 0,
                controller: 120,
                value: 0,
            },
        )),
        _ => None, // Keyboard bend is deliberately ignored, as in standalone.
    }
}
fn output(timing: u32, message: [u8; 3]) -> NoteEvent<()> {
    let [status, a, b] = message;
    let channel = status & 15;
    match status & 0xf0 {
        0x80 => NoteEvent::NoteOff {
            timing,
            voice_id: None,
            channel,
            note: a,
            velocity: f32::from(b) / 127.0,
        },
        0x90 => NoteEvent::NoteOn {
            timing,
            voice_id: None,
            channel,
            note: a,
            velocity: f32::from(b) / 127.0,
        },
        0xe0 => NoteEvent::MidiPitchBend {
            timing,
            channel,
            value: f32::from(u16::from(a) | (u16::from(b) << 7)) / 16383.0,
        },
        _ => NoteEvent::MidiCC {
            timing,
            channel,
            cc: a,
            value: f32::from(b) / 127.0,
        },
    }
}

impl Plugin for AdaptiveJiPlugin {
    const LEGACY_RT_STATE_RESTORE: bool = false;
    const NAME: &'static str = "adaptive-ji";
    const VENDOR: &'static str = "adaptive-ji";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[];
    const MIDI_INPUT: MidiConfig = MidiConfig::MidiCCs;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::MidiCCs;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;
    type SysExMessage = ();
    type BackgroundTask = EngineSettings;
    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }
    fn editor(&mut self, executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.control.clone(), executor)
    }
    fn task_executor(&mut self) -> TaskExecutor<Self> {
        let control = self.control.clone();
        let params = self.params.clone();
        Box::new(move |settings| {
            while control.retired.pop().is_some() {} // Destroy/join ONLY off RT.
            let rate = control.sample_rate.load(Ordering::Acquire);
            match Session::new(&settings, f64::from(f32::from_bits(rate))) {
                Ok(session) => {
                    *params.settings.lock().unwrap() = settings;
                    if control
                        .ready
                        .push(Box::new(PreparedSession { rate, session }))
                        .is_err()
                    {
                        *control.message.lock().unwrap() =
                            "A configuration is already pending".into();
                        control.busy.store(false, Ordering::Release);
                    } else {
                        *control.message.lock().unwrap() = "Prepared; applies at the next processing block (releases active notes)".into();
                    }
                }
                Err(error) => {
                    *control.message.lock().unwrap() = error;
                    control.busy.store(false, Ordering::Release);
                }
            }
        })
    }
    fn initialize(
        &mut self,
        _: &AudioIOLayout,
        config: &BufferConfig,
        context: &mut impl InitContext<Self>,
    ) -> bool {
        self.control
            .sample_rate
            .store(config.sample_rate.to_bits(), Ordering::Release);
        self.offline = config.process_mode == ProcessMode::Offline;
        while self.control.ready.pop().is_some() {}
        while self.control.retired.pop().is_some() {}
        self.control.busy.store(false, Ordering::Release);
        let settings = self.params.settings.lock().unwrap().clone();
        self.control.live.force_push(LiveSnapshot {
            attack_mode: settings.attack_mode,
            ..LiveSnapshot::default()
        });
        match Session::new(&settings, f64::from(config.sample_rate)) {
            Ok(session) => {
                self.active = Some(Box::new(PreparedSession {
                    rate: config.sample_rate.to_bits(),
                    session,
                }))
            }
            Err(error) => {
                *self.control.message.lock().unwrap() = error;
                return false;
            }
        }
        context.set_latency_samples(0);
        self.reset_pending = true;
        self.cursor = 0;
        self.expected_position = None;
        self.playing = None;
        true
    }
    fn reset(&mut self) {
        self.reset_pending = true;
        self.control.live.force_push(LiveSnapshot::default());
    }
    fn deactivate(&mut self) {
        self.reset_pending = true;
        self.control.live.force_push(LiveSnapshot::default());
    }
    fn process(
        &mut self,
        buffer: &mut Buffer,
        _: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let frames = buffer.samples() as u32;
        if frames == 0 {
            return ProcessStatus::Normal;
        }
        let start = self.cursor;
        let transport = context.transport();
        let position = transport.pos_samples();
        let playing = transport.playing;
        let discontinuity = self.playing.is_some_and(|old| old != playing)
            || (playing
                && self
                    .expected_position
                    .zip(position)
                    .is_some_and(|(a, b)| a != b));
        self.expected_position = position.map(|p| if playing { p + i64::from(frames) } else { p });
        self.playing = Some(playing);
        // RefCell only mediates sequential host next_event/send_event borrows;
        // it is stack-local, allocation-free and is not a mutex.
        let context = RefCell::new(context);
        let mut emit = |sample: u64, message| {
            context
                .borrow_mut()
                .send_event(output((sample - start) as u32, message))
        };
        if !self.control.retired.is_full()
            && let Some(new) = self.control.ready.pop()
        {
            if let Some(old) = &mut self.active {
                old.session.reset(start, self.offline, &mut emit);
            }
            let old = if new.rate == self.control.sample_rate.load(Ordering::Acquire) {
                self.active.replace(new)
            } else {
                Some(new)
            };
            if let Some(old) = old {
                let result = self.control.retired.push(old);
                debug_assert!(result.is_ok());
            }
            self.control.busy.store(false, Ordering::Release);
            self.reset_pending = true;
        }
        if let Some(active) = &mut self.active {
            if self.reset_pending
                || discontinuity
                || self.control.panic.swap(false, Ordering::AcqRel)
            {
                active.session.reset(start, self.offline, &mut emit);
                self.reset_pending = false;
            }
            let events = std::iter::from_fn(|| {
                loop {
                    let event = context.borrow_mut().next_event()?;
                    if let Some(event) = input(event) {
                        return Some(event);
                    }
                }
            });
            active
                .session
                .process(start, frames, self.offline, events, emit);
            self.control.live.force_push(
                active
                    .session
                    .core
                    .live_snapshot(start + u64::from(frames) - 1),
            );
        }
        self.cursor += u64::from(frames);
        ProcessStatus::Normal
    }
}
impl ClapPlugin for AdaptiveJiPlugin {
    const CLAP_ID: &'static str = "org.adaptive-ji.note-effect";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Adaptive rational tuning with per-note MPE output");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::NoteEffect, ClapFeature::Utility];
}
impl Vst3Plugin for AdaptiveJiPlugin {
    const VST3_CLASS_ID: [u8; 16] = *b"AdaptiveJI-MPE01";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Tools];
}
nih_export_clap!(AdaptiveJiPlugin);
nih_export_vst3!(AdaptiveJiPlugin);

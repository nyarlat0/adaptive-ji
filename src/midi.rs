use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
#[cfg(target_os = "linux")]
use midir::os::unix::{VirtualInput, VirtualOutput};
use midir::{
    Ignore, MidiInput, MidiInputConnection, MidiInputPort, MidiOutput, MidiOutputConnection,
};

use crate::{
    glide::GlideConfig,
    mpe::{MpeProcessor, PanicKind},
    pitch_bend::BendRange,
    tuning::TuningStrategy,
};

pub const INPUT_PORT_NAME: &str = "Adaptive JI In";
pub const OUTPUT_PORT_NAME: &str = "Adaptive JI Out";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiInputSource<'a> {
    Physical(&'a str),
    Virtual,
}

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
    bail!("adaptive-ji supports Linux/ALSA only")
}

enum LoopMessage {
    Midi(MidiEvent),
    Stop,
}

/// Coalesce only redundant global resets in one already-queued run. No timeout,
/// delayed panic, or persistent suppression: a panic after idle is always sent.
/// Every non-reset event is a barrier, including notes and pedal-down.
#[derive(Default)]
struct ResetBurst {
    panic: bool,
    controllers: bool,
    pedal_up: bool,
}

impl ResetBurst {
    fn accept(&mut self, event: MidiEvent) -> bool {
        match event {
            MidiEvent::ControlChange {
                controller: 120 | 123,
                value: 0,
                ..
            } => {
                let accept = !self.panic;
                // Both processor paths send CC120 AND CC123 on all channels.
                self.panic = true;
                self.pedal_up = true;
                accept
            }
            MidiEvent::ControlChange {
                controller: 121,
                value: 0,
                ..
            } => {
                let accept = !self.controllers;
                self.controllers = true;
                self.pedal_up = true;
                accept
            }
            MidiEvent::ControlChange {
                controller: 64,
                value: 0,
                ..
            } => {
                let accept = !self.pedal_up;
                self.pedal_up = true;
                accept
            }
            _ => {
                *self = Self::default();
                true
            }
        }
    }
}

/// Drain a bounded amount of available work, never waiting for a burst to fill.
/// Returns true on Stop. Failed sends abort immediately; shutdown still retries
/// the processor's retained voice identities through the normal panic path.
fn process_ready_batch<T: TuningStrategy, E>(
    processor: &mut MpeProcessor<T>,
    first: LoopMessage,
    mut next: impl FnMut() -> Option<LoopMessage>,
    mut now: impl FnMut() -> Duration,
    mut send: impl FnMut(&[u8; 3]) -> Result<(), E>,
) -> Result<bool, E> {
    const MAX_BATCH: usize = 256;
    let mut resets = ResetBurst::default();
    let mut message = first;
    let mut coalesced = 0;
    for index in 0..MAX_BATCH {
        let LoopMessage::Midi(event) = message else {
            return Ok(true);
        };
        let timestamp = now();
        log::debug!(
            "MIDI IN t={}us {event:?} (channels zero-based)",
            timestamp.as_micros()
        );
        if resets.accept(event) {
            processor.process_at_to(event, timestamp, &mut send)?;
        } else {
            coalesced += 1;
            log::debug!("MIDI IN coalesced redundant reset {event:?}");
        }
        // Busy inputs cannot starve glide ticks. Dead voices are removed first.
        for bend in processor.tick_at(now()) {
            send(&bend)?;
        }
        if index + 1 == MAX_BATCH {
            break;
        }
        let Some(queued) = next() else { break };
        message = queued;
    }
    if coalesced != 0 {
        log::info!("RESET BURST coalesced={coalesced} redundant queued commands");
    }
    Ok(false)
}

fn send_output(
    output: &mut MidiOutputConnection,
    message: &[u8; 3],
    clock_start: Instant,
) -> std::result::Result<(), midir::SendError> {
    let result = output.send(message);
    match &result {
        Ok(()) => log::debug!(
            "MIDI OUT t={}us sent={message:02x?}",
            clock_start.elapsed().as_micros()
        ),
        Err(error) => log::error!("MIDI OUT failed={message:02x?}: {error}"),
    }
    result
}

struct MidiIngress {
    sender: SyncSender<LoopMessage>,
    overflow: Arc<AtomicBool>,
}

// Both midir connection types use this exact callback. Parsing and queueing
// are independent of the backend; tuning remains on the single event loop.
fn receive_midi(_timestamp: u64, bytes: &[u8], ingress: &mut MidiIngress) {
    if let Some(event) = parse_message(bytes)
        && ingress.sender.try_send(LoopMessage::Midi(event)).is_err()
    {
        ingress.overflow.store(true, Ordering::Relaxed);
    }
}

/// The only backend-dependent operations. A fake can exercise source selection
/// and the real ingress callback without opening /dev/snd/seq.
trait InputBackend: Sized {
    type Port;
    type Connection;

    fn sources(&self) -> Result<Vec<(Self::Port, String)>>;
    fn connect_physical(self, port: &Self::Port, ingress: MidiIngress) -> Result<Self::Connection>;
    fn create_destination(self, ingress: MidiIngress) -> Result<Self::Connection>;
}

impl InputBackend for MidiInput {
    type Port = MidiInputPort;
    type Connection = MidiInputConnection<MidiIngress>;

    fn sources(&self) -> Result<Vec<(Self::Port, String)>> {
        self.ports()
            .into_iter()
            .map(|port| self.port_name(&port).map(|name| (port, name)))
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("Cannot read MIDI input port names")
    }

    fn connect_physical(self, port: &Self::Port, ingress: MidiIngress) -> Result<Self::Connection> {
        self.connect(port, "adaptive-ji keyboard", receive_midi, ingress)
            .map_err(|error| anyhow!("Cannot connect MIDI input: {error}"))
    }

    #[cfg(target_os = "linux")]
    fn create_destination(self, ingress: MidiIngress) -> Result<Self::Connection> {
        // ALSA WRITE | SUBS_WRITE: other clients SEND to this destination.
        // No subscriptions are created here; routing belongs to Ardour/ALSA.
        self.create_virtual(INPUT_PORT_NAME, receive_midi, ingress)
            .map_err(|error| {
                anyhow!("Cannot create virtual ALSA input {INPUT_PORT_NAME:?}: {error}")
            })
    }

    #[cfg(not(target_os = "linux"))]
    fn create_destination(self, _ingress: MidiIngress) -> Result<Self::Connection> {
        bail!("Virtual MIDI input requires Linux/ALSA")
    }
}

struct PreparedInput<B: InputBackend> {
    backend: B,
    port: Option<B::Port>,
    name: String,
}

impl<B: InputBackend> PreparedInput<B> {
    fn new(source: MidiInputSource<'_>, backend: B) -> Result<Self> {
        let (port, name) = match source {
            MidiInputSource::Physical(selector) => {
                let mut ports = backend.sources()?;
                let names = ports
                    .iter()
                    .map(|(_, name)| name.clone())
                    .collect::<Vec<_>>();
                let index = select_input(&names, selector)?;
                let (port, name) = ports.swap_remove(index);
                (Some(port), name)
            }
            MidiInputSource::Virtual => (None, INPUT_PORT_NAME.to_owned()),
        };
        Ok(Self {
            backend,
            port,
            name,
        })
    }

    fn connect(self, ingress: MidiIngress) -> Result<B::Connection> {
        match self.port {
            Some(port) => self.backend.connect_physical(&port, ingress),
            None => self.backend.create_destination(ingress),
        }
        .with_context(|| format!("Cannot open MIDI input {:?}", self.name))
    }
}

pub fn run<T: TuningStrategy>(
    selector: &str,
    range: BendRange,
    tuning: T,
    glide: GlideConfig,
) -> Result<()> {
    run_with_source(MidiInputSource::Physical(selector), range, tuning, glide)
}

pub fn run_with_source<T: TuningStrategy>(
    source: MidiInputSource<'_>,
    range: BendRange,
    tuning: T,
    glide: GlideConfig,
) -> Result<()> {
    let mut input = input_client()?;
    input.ignore(Ignore::None);
    let input = PreparedInput::new(source, input)?;
    let input_name = input.name.clone();
    let mut output = virtual_output()?;
    let mut processor = MpeProcessor::with_glide(tuning, range, glide);

    // One bounded queue and one sender thread: the same loop processes input,
    // optimizer retunes, and due glide bends in strict MIDI output order.
    let (sender, receiver) = mpsc::sync_channel(1024);

    let stopping = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&stopping);
    let signal_sender = sender.clone();
    ctrlc::set_handler(move || {
        signal.store(true, Ordering::Relaxed);
        let _ = signal_sender.try_send(LoopMessage::Stop);
    })
    .context("Cannot install Ctrl-C handler")?;

    // The callback never waits for processing/output. Overflow is fatal: silently
    // dropping an arbitrary NoteOff could leave a sounding voice behind.
    let overflow = Arc::new(AtomicBool::new(false));
    let connection = input.connect(MidiIngress {
        sender,
        overflow: Arc::clone(&overflow),
    })?;

    match source {
        MidiInputSource::Physical(_) => eprintln!(
            "{input_name} -> {OUTPUT_PORT_NAME}; bend range ±{} semitones. Ctrl-C to stop.",
            range.semitones()
        ),
        MidiInputSource::Virtual => {
            eprintln!("MIDI input:  {INPUT_PORT_NAME} (virtual ALSA port)");
            eprintln!("MIDI output: {OUTPUT_PORT_NAME}");
            eprintln!(
                "Bend range ±{} semitones. Ctrl-C to stop.",
                range.semitones()
            );
        }
    }
    let clock_start = Instant::now();
    let result = (|| -> Result<()> {
        for message in processor.initialize() {
            send_output(&mut output, &message, clock_start)
                .context("Failed to initialize MPE channels")?;
        }
        while !stopping.load(Ordering::Relaxed) {
            if overflow.load(Ordering::Relaxed) {
                bail!(
                    "MIDI input queue overflow; stopping and releasing voices to avoid stuck notes"
                );
            }
            let received = if let Some(deadline) = processor.next_glide_deadline() {
                receiver.recv_timeout(deadline.saturating_sub(clock_start.elapsed()))
            } else {
                receiver.recv().map_err(|_| RecvTimeoutError::Disconnected)
            };
            match received {
                Ok(first) => {
                    if process_ready_batch(
                        &mut processor,
                        first,
                        || receiver.try_recv().ok(),
                        || clock_start.elapsed(),
                        |message| send_output(&mut output, message, clock_start),
                    )
                    .context("Failed to send MIDI output")?
                    {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    for message in processor.tick_at(clock_start.elapsed()) {
                        send_output(&mut output, &message, clock_start)
                            .context("Failed to send glide Pitch Bend")?;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => bail!("MIDI input callback disconnected"),
            }
        }
        Ok(())
    })();

    // Stop the callback before draining state. Attempt every cleanup message even
    // when one send fails; a failed output must not prevent the other channels' reset.
    connection.close();
    let cleanup_result = processor.panic_output(PanicKind::Shutdown, |message| {
        send_output(&mut output, message, clock_start).map_err(|error| {
            log::error!("MIDI shutdown send failed: {error}");
            anyhow!("MIDI shutdown incomplete: {error}")
        })
    });
    output.close();
    result?;
    cleanup_result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        pitch_bend::{CENTER, encode_pitch_bend},
        tuning::{
            AdaptiveRationalOptimizer, OptimizerConfig, PrimeLimit,
            profile::{SpectrumPreset, SpectrumProfile},
            scala::{KeyboardMapping, ScalaScale, ScalaTuning},
        },
        voice::VoiceState,
    };
    use std::time::Duration;

    struct FakeBackend {
        allow_source_discovery: bool,
    }

    struct FakeConnection {
        virtual_input: bool,
        ingress: MidiIngress,
    }

    impl InputBackend for FakeBackend {
        type Port = u8;
        type Connection = FakeConnection;

        fn sources(&self) -> Result<Vec<(u8, String)>> {
            assert!(
                self.allow_source_discovery,
                "virtual mode must not enumerate sources"
            );
            Ok(vec![(42, "KeyRig 49 MIDI".into())])
        }

        fn connect_physical(self, port: &u8, ingress: MidiIngress) -> Result<FakeConnection> {
            assert_eq!(*port, 42);
            Ok(FakeConnection {
                virtual_input: false,
                ingress,
            })
        }

        fn create_destination(self, ingress: MidiIngress) -> Result<FakeConnection> {
            Ok(FakeConnection {
                virtual_input: true,
                ingress,
            })
        }
    }

    fn fake_input(
        source: MidiInputSource<'_>,
        capacity: usize,
    ) -> (FakeConnection, mpsc::Receiver<LoopMessage>) {
        let input = PreparedInput::new(
            source,
            FakeBackend {
                allow_source_discovery: matches!(source, MidiInputSource::Physical(_)),
            },
        )
        .unwrap();
        if source == MidiInputSource::Virtual {
            assert_eq!(input.name, INPUT_PORT_NAME);
        }
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let connection = input
            .connect(MidiIngress {
                sender,
                overflow: Arc::new(AtomicBool::new(false)),
            })
            .unwrap();
        assert_eq!(connection.virtual_input, source == MidiInputSource::Virtual);
        (connection, receiver)
    }

    fn test_processor(scala: bool, duration_ms: u64) -> MpeProcessor<AdaptiveRationalOptimizer> {
        let config = OptimizerConfig {
            prime_limit: PrimeLimit::new(11).unwrap(),
            max_deviation_cents: 50.0,
            ..OptimizerConfig::default()
        };
        let mapping = scala.then(|| {
            ScalaTuning::new(
                ScalaScale::parse(include_str!("../scales/13_equal_div_3_1.scl")).unwrap(),
                Some(
                    KeyboardMapping::parse(include_str!("../scales/13_equal_div_3_1.kbm")).unwrap(),
                ),
                60,
                2.0,
            )
            .unwrap()
            .with_adaptive_headroom(2.0, config.max_deviation_cents)
            .unwrap()
        });
        let profile = SpectrumProfile::builtin(SpectrumPreset::Harmonic, config.roughness).unwrap();
        MpeProcessor::with_glide(
            AdaptiveRationalOptimizer::with_profile_and_scale(config, profile, mapping).unwrap(),
            "2".parse().unwrap(),
            GlideConfig {
                duration_ms,
                tick_ms: 5,
            },
        )
    }

    fn queued_output<T: TuningStrategy>(
        processor: &mut MpeProcessor<T>,
        bytes: &[[u8; 3]],
    ) -> Vec<[u8; 3]> {
        let mut events = bytes
            .iter()
            .map(|bytes| LoopMessage::Midi(parse_message(bytes).unwrap()));
        let mut output = Vec::new();
        while let Some(first) = events.next() {
            assert!(
                !process_ready_batch(
                    processor,
                    first,
                    || events.next(),
                    || Duration::ZERO,
                    |message| {
                        output.push(*message);
                        Ok::<_, std::convert::Infallible>(())
                    },
                )
                .unwrap()
            );
        }
        output
    }

    #[test]
    fn queued_panic_sweep_is_not_multiplied_by_sixteen() {
        // Ardour's midi_panic sends sustain-up, notes-off and controller-reset
        // for each of 16 input channels. These are global operations here.
        let burst: Vec<_> = (0..16)
            .flat_map(|channel| [64, 123, 121].map(|cc| [0xb0 | channel, cc, 0]))
            .collect();
        let mut processor = test_processor(true, 35);
        processor.process(parse_message(&[0x90, 45, 100]).unwrap());
        processor.process(parse_message(&[0x90, 48, 100]).unwrap());
        let allocated = processor.voices().to_vec();
        assert!(allocated.iter().all(|v| v.input_note != v.output_note));
        let output = queued_output(&mut processor, &burst);
        assert_eq!(output.len(), 125); // 123 reset messages plus two mapped offs.
        for voice in allocated {
            assert_eq!(
                output
                    .iter()
                    .filter(|m| **m == [0x80 | voice.output_channel, voice.output_note, 0])
                    .count(),
                1
            );
        }
        assert_eq!(output.iter().filter(|m| **m == [0xb0, 123, 0]).count(), 1);
        assert!(processor.voices().is_empty());
        assert!(processor.next_glide_deadline().is_none());
        assert!(processor.tick_at(Duration::from_secs(1)).is_empty());

        // Independent later panics still recover an empty/desynchronized synth.
        for cc in [123, 120, 123] {
            let output = queued_output(&mut processor, &[[0xb0, cc, 0]]);
            for channel in 0..16 {
                assert!(output.contains(&[0xb0 | channel, 120, 0]));
                assert!(output.contains(&[0xb0 | channel, 123, 0]));
            }
        }
    }

    #[test]
    fn reset_coalescing_cannot_cross_notes_or_other_controllers() {
        let mut processor = test_processor(false, 0);
        let burst = [
            [0xb0, 123, 0],
            [0xb1, 123, 0],
            [0x90, 60, 100],
            [0xb2, 123, 0],
            [0xb3, 120, 0],
            [0x90, 60, 100],
            [0x80, 60, 0],
            [0xb4, 123, 0],
            [0xb0, 64, 127],
            [0xb5, 123, 0],
            [0xb0, 1, 80],
            [0xb6, 123, 0],
        ];
        let output = queued_output(&mut processor, &burst);
        assert_eq!(output.iter().filter(|m| **m == [0xb0, 123, 0]).count(), 5);
        assert_eq!(output.iter().filter(|m| **m == [0x81, 60, 0]).count(), 2);
        assert_eq!(output.iter().filter(|m| **m == [0x91, 60, 100]).count(), 2);
        assert!(processor.voices().is_empty());
    }

    #[test]
    fn queued_sustain_reset_keeps_held_voice_and_matches_unbatched_state() {
        for scala in [false, true] {
            let mut batched = test_processor(scala, 35);
            let mut unbatched = test_processor(scala, 35);
            let mut sequence = vec![
                [0x90, 45, 100],
                [0x90, 48, 100],
                [0xb0, 64, 127],
                [0x80, 45, 0],
            ];
            sequence.extend((0..16).flat_map(|ch| [[0xb0 | ch, 121, 0], [0xb0 | ch, 64, 0]]));
            let output = queued_output(&mut batched, &sequence);
            for bytes in sequence {
                unbatched.process(parse_message(&bytes).unwrap());
            }
            assert_eq!(batched.voices().len(), 1);
            assert_eq!(batched.voices()[0].input_note, 48);
            assert_eq!(batched.voices()[0].state, VoiceState::Held);
            assert_eq!(
                batched.voices()[0].cents_offset,
                unbatched.voices()[0].cents_offset
            );
            assert_eq!(
                batched.tick_at(Duration::from_secs(1)),
                unbatched.tick_at(Duration::from_secs(1))
            );
            assert_eq!(output.iter().filter(|m| **m == [0xb1, 121, 0]).count(), 1);
        }
    }

    #[test]
    fn ready_batch_is_bounded_and_never_consumes_past_stop_or_send_failure() {
        let mut processor = test_processor(false, 0);
        let panic = || LoopMessage::Midi(parse_message(&[0xb0, 123, 0]).unwrap());
        let mut polled = 0;
        process_ready_batch(
            &mut processor,
            panic(),
            || {
                polled += 1;
                Some(panic())
            },
            || Duration::ZERO,
            |_| Ok::<_, ()>(()),
        )
        .unwrap();
        assert_eq!(polled, 255);
        assert!(
            process_ready_batch(
                &mut processor,
                LoopMessage::Stop,
                || panic!("read past Stop"),
                || Duration::ZERO,
                |_| Ok::<_, ()>(())
            )
            .unwrap()
        );
        processor.process(parse_message(&[0x90, 60, 100]).unwrap());
        assert!(
            process_ready_batch(
                &mut processor,
                panic(),
                || panic!("read past failed send"),
                || Duration::ZERO,
                |_| Err(())
            )
            .is_err()
        );
        assert_eq!(processor.voices().len(), 1);
    }

    #[test]
    fn physical_and_virtual_ingress_match_existing_adaptive_scala_sustain_and_glide_pipeline() {
        let sequence = [
            [0x90, 60, 100],
            [0x90, 64, 100],
            [0x90, 67, 100],
            [0xb0, 64, 127],
            [0x80, 60, 45],
            [0x90, 60, 110],
            [0x90, 60, 0],
            [0x90, 70, 100],
            [0xb0, 64, 0],
            [0x80, 64, 23],
            [0x80, 67, 0],
            [0x80, 70, 0],
            [0xb0, 123, 0],
            [0x90, 69, 100],
            [0xb0, 120, 0],
            [0x90, 60, 100],
            [0x90, 64, 100],
            [0xb0, 64, 127],
            [0x80, 64, 0],
            [0xb0, 121, 0],
        ];
        for scala in [false, true] {
            for duration_ms in [0, 35] {
                let mut legacy = test_processor(scala, duration_ms);
                let mut physical = test_processor(scala, duration_ms);
                let mut virtual_input = test_processor(scala, duration_ms);
                let (mut hardware, hardware_queue) =
                    fake_input(MidiInputSource::Physical("KeyRig"), 8);
                let (mut destination, destination_queue) = fake_input(MidiInputSource::Virtual, 8);
                for (index, bytes) in sequence.iter().enumerate() {
                    let now = Duration::from_millis(index as u64 * 5);
                    let expected = legacy.process_at(parse_message(bytes).unwrap(), now);
                    for (connection, queue, processor) in [
                        (&mut hardware, &hardware_queue, &mut physical),
                        (&mut destination, &destination_queue, &mut virtual_input),
                    ] {
                        receive_midi(0, bytes, &mut connection.ingress);
                        let LoopMessage::Midi(event) = queue.try_recv().unwrap() else {
                            panic!("expected MIDI event")
                        };
                        assert_eq!(processor.process_at(event, now), expected);
                        assert!(!connection.ingress.overflow.load(Ordering::Relaxed));
                    }
                    if index == 4 {
                        assert_eq!(expected, [[0x81, 60, 45]]);
                        assert_eq!(virtual_input.voices().len(), 3);
                        assert_eq!(
                            virtual_input.voices()[0].state,
                            VoiceState::ReleasedSustained
                        );
                    }
                    if index == 6 {
                        assert_eq!(expected, [[0x84, 60, 0]]);
                        assert_eq!(virtual_input.voices().len(), 4);
                    }
                    if index == 8 {
                        assert_eq!(expected[0], [0xb1, 64, 0]);
                        assert_eq!(expected[14], [0xbf, 64, 0]);
                        assert_eq!(expected[15], [0x81, 60, 0]);
                        assert_eq!(expected[16], [0x84, 60, 0]);
                        assert_eq!(expected[17], encode_pitch_bend(1, CENTER));
                        assert_eq!(expected[18], encode_pitch_bend(4, CENTER));
                        assert_eq!(virtual_input.voices().len(), 3);
                    }
                    if index == 9 {
                        assert_eq!(expected[0], [0x82, if scala { 66 } else { 64 }, 23]);
                    }
                    let tick = now + Duration::from_millis(2);
                    let expected = legacy.tick_at(tick);
                    assert_eq!(physical.tick_at(tick), expected);
                    assert_eq!(virtual_input.tick_at(tick), expected);
                }
                let cleanup = legacy.shutdown();
                assert_eq!(physical.shutdown(), cleanup);
                assert_eq!(virtual_input.shutdown(), cleanup);
                assert!(virtual_input.voices().is_empty());
                assert!(virtual_input.next_glide_deadline().is_none());
                assert!(virtual_input.tick_at(Duration::from_secs(1)).is_empty());
            }
        }
    }

    #[test]
    fn shared_ingress_ignores_unsupported_messages_and_reports_queue_overflow() {
        for source in [MidiInputSource::Physical("0"), MidiInputSource::Virtual] {
            let (mut connection, queue) = fake_input(source, 1);
            for bytes in [&[0xe0, 0, 64][..], &[0xf8], &[0x90, 60], &[0xf0, 1, 0xf7]] {
                receive_midi(0, bytes, &mut connection.ingress);
                assert!(queue.try_recv().is_err());
            }
            receive_midi(0, &[0x90, 60, 100], &mut connection.ingress);
            receive_midi(0, &[0x80, 60, 0], &mut connection.ingress);
            assert!(connection.ingress.overflow.load(Ordering::Relaxed));
        }
    }

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

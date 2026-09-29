use super::*;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    collections::VecDeque,
};
struct Alloc;
thread_local! {static COUNT:Cell<Option<usize>>=const{Cell::new(None)};}
unsafe impl GlobalAlloc for Alloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        COUNT.with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + 1));
            }
        });
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        COUNT.with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + 1));
            }
        });
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static ALLOC: Alloc = Alloc;
struct Host {
    transport: Transport,
    input: VecDeque<NoteEvent<()>>,
    output: Vec<NoteEvent<()>>,
}
impl ProcessContext<AdaptiveJiPlugin> for Host {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
    }
    fn execute_background(&self, _: EngineSettings) {
        panic!("callback scheduled configuration")
    }
    fn execute_gui(&self, _: EngineSettings) {
        panic!("callback scheduled GUI")
    }
    fn transport(&self) -> &Transport {
        &self.transport
    }
    fn next_event(&mut self) -> Option<NoteEvent<()>> {
        self.input.pop_front()
    }
    fn send_event(&mut self, e: NoteEvent<()>) {
        self.output.push(e);
    }
    fn set_latency_samples(&self, samples: u32) {
        assert_eq!(samples, 0);
    }
    fn set_current_voice_capacity(&self, _: u32) {}
}
#[test]
fn plugin_callback_allocates_nothing_and_preserves_offsets_and_stop_panic() {
    let mut plugin = AdaptiveJiPlugin {
        active: Some(Box::new(PreparedSession {
            rate: 48000.0f32.to_bits(),
            session: Session::new(&EngineSettings::default(), 48000.0).unwrap(),
        })),
        ..AdaptiveJiPlugin::default()
    };
    let mut host = Host {
        transport: Transport::new(48000.0),
        input: (48..63)
            .map(|note| NoteEvent::NoteOn {
                timing: 19,
                voice_id: None,
                channel: 0,
                note,
                velocity: 0.8,
            })
            .collect(),
        output: Vec::with_capacity(2048),
    };
    host.transport.playing = true;
    let mut buffer = Buffer::default();
    // A note-only plugin still receives the block's sample count, with no slices.
    unsafe {
        buffer.set_slices(64, |_| {});
    }
    COUNT.with(|c| c.set(Some(0)));
    plugin.process(
        &mut buffer,
        &mut AuxiliaryBuffers {
            inputs: &mut [],
            outputs: &mut [],
        },
        &mut host,
    );
    let allocations = COUNT.with(|c| c.replace(None).unwrap());
    assert_eq!(allocations, 0);
    assert_eq!(plugin.active.as_ref().unwrap().session.core.generation(), 1);
    let on_count = host
        .output
        .iter()
        .filter(|e| matches!(e, NoteEvent::NoteOn { timing: 19, .. }))
        .count();
    assert_eq!(on_count, 15);
    host.output.clear();
    host.transport.playing = false;
    COUNT.with(|c| c.set(Some(0)));
    plugin.process(
        &mut buffer,
        &mut AuxiliaryBuffers {
            inputs: &mut [],
            outputs: &mut [],
        },
        &mut host,
    );
    let allocations = COUNT.with(|c| c.replace(None).unwrap());
    assert_eq!(allocations, 0);
    assert!(
        plugin
            .active
            .as_ref()
            .unwrap()
            .session
            .core
            .voices()
            .is_empty()
    );
    assert_eq!(
        host.output
            .iter()
            .filter(|e| matches!(e, NoteEvent::NoteOff { timing: 0, .. }))
            .count(),
        15
    );
}

#[test]
fn midi_conversion_preserves_mapped_notes_and_bend_quantization() {
    for value in [0, 1, 7631, 8192, 9552, 16383] {
        let bytes = adaptive_ji_core::pitch_bend::encode_pitch_bend(5, value);
        let NoteEvent::MidiPitchBend {
            timing,
            channel,
            value: normalized,
        } = output(37, bytes)
        else {
            panic!()
        };
        assert_eq!((timing, channel), (37, 5));
        assert_eq!((normalized * 16383.0).round() as u16, value);
    }
    let NoteEvent::NoteOff {
        timing,
        channel,
        note,
        ..
    } = output(23, [0x85, 38, 0])
    else {
        panic!()
    };
    assert_eq!((timing, channel, note), (23, 5, 38));
}

#[test]
fn format_frontend_and_portable_session_emit_identical_offline_mpe() {
    let settings = EngineSettings::default();
    let mut plugin = AdaptiveJiPlugin {
        active: Some(Box::new(PreparedSession {
            rate: 48000.0f32.to_bits(),
            session: Session::new(&settings, 48000.0).unwrap(),
        })),
        offline: true,
        ..AdaptiveJiPlugin::default()
    };
    let mut session = Session::new(&settings, 48000.0).unwrap();
    let messages = [
        (7, [0x90, 60, 100]),
        (7, [0x90, 64, 100]),
        (7, [0x90, 67, 100]),
        (20, [0xb0, 64, 127]),
        (21, [0x80, 60, 0]),
        (23, [0x90, 60, 100]),
        (41, [0xb0, 64, 0]),
        (55, [0xb0, 123, 0]),
    ];
    let mut expected = Vec::new();
    session.process(
        0,
        64,
        true,
        messages
            .iter()
            .map(|&(time, m)| input(output(time, m)).unwrap()),
        |s, m| expected.push(output(s as u32, m)),
    );
    let mut host = Host {
        transport: Transport::new(48000.0),
        input: messages.iter().map(|&(t, m)| output(t, m)).collect(),
        output: Vec::with_capacity(512),
    };
    let mut buffer = Buffer::default();
    unsafe {
        buffer.set_slices(64, |_| {});
    }
    plugin.process(
        &mut buffer,
        &mut AuxiliaryBuffers {
            inputs: &mut [],
            outputs: &mut [],
        },
        &mut host,
    );
    assert_eq!(host.output, expected);
}

#[test]
fn tuned_plugin_attacks_at_solved_bends_and_publishes_readable_live_data() {
    use adaptive_ji_core::{pitch_bend::cents_to_pitch_bend, settings::AttackMode};
    let settings = EngineSettings {
        attack_mode: AttackMode::Tuned,
        ..EngineSettings::default()
    };
    let mut plugin = AdaptiveJiPlugin {
        active: Some(Box::new(PreparedSession {
            rate: 48000.0f32.to_bits(),
            session: Session::new(&settings, 48000.0).unwrap(),
        })),
        offline: true,
        ..AdaptiveJiPlugin::default()
    };
    let mut host = Host {
        transport: Transport::new(48000.0),
        input: [60, 64, 67]
            .into_iter()
            .map(|n| output(19, [0x90, n, 100]))
            .collect(),
        output: Vec::with_capacity(1024),
    };
    let mut buffer = Buffer::default();
    unsafe {
        buffer.set_slices(2048, |_| {});
    }
    plugin.process(
        &mut buffer,
        &mut AuxiliaryBuffers {
            inputs: &mut [],
            outputs: &mut [],
        },
        &mut host,
    );
    let snapshot = plugin.control.live.pop().unwrap();
    assert_eq!(snapshot.attack_mode, AttackMode::Tuned);
    assert!(!snapshot.optimizing);
    assert_eq!(snapshot.voices.iter().flatten().count(), 3);
    for voice in snapshot.voices.iter().flatten() {
        let target = voice.target.unwrap();
        assert!(!voice.pending);
        assert_eq!(voice.current_cents, Some(target.cents_offset));
        let on=host.output.iter().position(|e| matches!(e, NoteEvent::NoteOn {timing:19,channel,note,..} if *channel==voice.output_channel && *note==voice.output_note)).unwrap();
        let bend = host.output[..on]
            .iter()
            .rev()
            .find_map(|e| match e {
                NoteEvent::MidiPitchBend {
                    timing: 19,
                    channel,
                    value,
                } if *channel == voice.output_channel => Some((value * 16383.0).round() as u16),
                _ => None,
            })
            .unwrap();
        assert_eq!(bend, cents_to_pitch_bend(target.cents_offset, 2.0));
    }
    // No attack glide ticks during the remainder of this long block.
    assert!(
        host.output
            .iter()
            .all(|e| !matches!(e,NoteEvent::MidiPitchBend {timing,..} if *timing>19))
    );
    plugin.deactivate();
    assert!(
        plugin
            .control
            .live
            .pop()
            .unwrap()
            .voices
            .iter()
            .all(Option::is_none)
    );
}

#[test]
fn tuned_realtime_plugin_pending_notes_and_telemetry_allocate_nothing() {
    let settings = EngineSettings {
        attack_mode: adaptive_ji_core::settings::AttackMode::Tuned,
        ..EngineSettings::default()
    };
    let mut plugin = AdaptiveJiPlugin {
        active: Some(Box::new(PreparedSession {
            rate: 48000.0f32.to_bits(),
            session: Session::new(&settings, 48000.0).unwrap(),
        })),
        ..AdaptiveJiPlugin::default()
    };
    let mut host = Host {
        transport: Transport::new(48000.0),
        input: (48..63).map(|n| output(19, [0x90, n, 100])).collect(),
        output: Vec::with_capacity(1024),
    };
    let mut buffer = Buffer::default();
    unsafe {
        buffer.set_slices(64, |_| {});
    }
    COUNT.with(|c| c.set(Some(0)));
    plugin.process(
        &mut buffer,
        &mut AuxiliaryBuffers {
            inputs: &mut [],
            outputs: &mut [],
        },
        &mut host,
    );
    let count = COUNT.with(|c| c.replace(None).unwrap());
    assert_eq!(count, 0);
    assert!(
        !host
            .output
            .iter()
            .any(|e| matches!(e, NoteEvent::NoteOn { .. }))
    );
    let snapshot = plugin.control.live.pop().unwrap();
    assert_eq!(
        snapshot
            .voices
            .iter()
            .flatten()
            .filter(|v| v.pending && v.current_cents.is_none())
            .count(),
        15
    );
}

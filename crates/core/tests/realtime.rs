use adaptive_ji_core::{
    event::MidiEvent,
    realtime::{RealtimeCore, RetuneResult},
    settings::{EmbeddedScale, EngineSettings},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

struct Counting;
thread_local! { static COUNT: Cell<Option<usize>> = const { Cell::new(None) }; }
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNT.with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        unsafe { System.dealloc(p, layout) }
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        COUNT.with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + 1));
            }
        });
        unsafe { System.realloc(p, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
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
        velocity: 0,
    }
}
fn cc(controller: u8, value: u8) -> MidiEvent {
    MidiEvent::ControlChange {
        channel: 0,
        controller,
        value,
    }
}

#[test]
fn callback_has_zero_allocations_including_first_chord_sustain_glide_and_panic() {
    let settings = EngineSettings::default();
    let prepared = settings.prepare().unwrap();
    let mut core = RealtimeCore::new(&prepared, 48000.0, true).unwrap();
    COUNT.with(|c| c.set(Some(0)));
    core.begin_batch();
    for note in 48..63 {
        core.event(7, on(note), |_, _| {});
    }
    let request = core.finish_batch(7).unwrap();
    let mut result = RetuneResult::from_targets(request, &[]);
    result.len = 15;
    for (target, note) in result.targets.iter_mut().zip(request.notes) {
        target.id = note.id;
        target.cents_offset = -13.0;
    }
    assert!(core.apply_result(8, result, |_, _| {}));
    core.advance_before(500, |_, _| {});
    core.begin_batch();
    core.event(500, cc(64, 127), |_, _| {});
    for note in 48..63 {
        core.event(500, off(note), |_, _| {});
    }
    core.finish_batch(500);
    core.advance_before(1000, |_, _| {});
    core.begin_batch();
    core.event(1000, cc(64, 0), |_, _| {});
    core.finish_batch(1000);
    core.reset(1001, |_, _| {});
    core.advance_before(10000, |_, _| panic!("stale glide"));
    let count = COUNT.with(|c| c.replace(None).unwrap());
    assert_eq!(count, 0);
}

#[test]
fn simultaneous_chord_is_one_request_and_stale_results_cannot_resurrect_voices() {
    let prepared = EngineSettings::default().prepare().unwrap();
    let mut core = RealtimeCore::new(&prepared, 48000.0, true).unwrap();
    let mut output = Vec::new();
    core.begin_batch();
    for note in [60, 64, 67] {
        core.event(23, on(note), |s, m| output.push((s, m)));
    }
    let request = core.finish_batch(23).unwrap();
    assert_eq!(request.len, 3);
    assert_eq!(request.generation, 1);
    assert!(
        request.notes[..3]
            .iter()
            .all(|n| n.current_offset_cents.is_none())
    );
    assert!(core.finish_batch(23).is_none());
    assert!(output.iter().all(|(s, _)| *s == 23));
    for (index, (_, m)) in output.iter().enumerate() {
        if m[0] & 0xf0 == 0x90 {
            assert_eq!(output[index - 1].1[0], 0xe0 | (m[0] & 15));
        }
    }
    core.reset(24, |_, _| {});
    assert!(
        !core.apply_result(25, RetuneResult::from_targets(request, &[]), |_, _| panic!(
            "stale result"
        ))
    );
}

#[test]
fn same_sample_channel_reset_sweep_is_not_multiplied_by_sixteen() {
    let prepared = EngineSettings::default().prepare().unwrap();
    let mut core = RealtimeCore::new(&prepared, 48000.0, true).unwrap();
    core.begin_batch();
    core.event(0, on(60), |_, _| {});
    core.finish_batch(0);
    core.begin_batch();
    let mut count = 0;
    for channel in 0..16 {
        core.event(
            64,
            MidiEvent::ControlChange {
                channel,
                controller: 123,
                value: 0,
            },
            |_, _| count += 1,
        );
    }
    assert!(count <= 80, "reset storm amplified: {count}");
    assert!(core.voices().is_empty());
    assert_eq!(core.finish_batch(64).unwrap().generation, 2);
    // A new note between panic messages must not be protected by coalescing.
    core.event(64, on(60), |_, _| {});
    core.event(64, cc(123, 0), |_, _| {});
    assert!(core.voices().is_empty());
}

fn render(block: u64, settings: &EngineSettings) -> Vec<(u64, [u8; 3])> {
    let mut prepared = settings.prepare().unwrap();
    let mut core = RealtimeCore::new(&prepared, 48000.0, true).unwrap();
    let events = [
        (17, on(60)),
        (17, on(64)),
        (17, on(67)),
        (800, cc(64, 127)),
        (900, off(60)),
        (901, on(60)),
        (1003, on(70)),
        (1100, cc(64, 0)),
        (3000, cc(123, 0)),
    ];
    let mut index = 0;
    let mut output = Vec::new();
    for start in (0..4096).step_by(block as usize) {
        let end = (start + block).min(4096);
        while index < events.len() && events[index].0 < end {
            let sample = events[index].0;
            core.advance_before(sample, |s, m| output.push((s, m)));
            core.begin_batch();
            while index < events.len() && events[index].0 == sample {
                core.event(sample, events[index].1, |s, m| output.push((s, m)));
                index += 1;
            }
            if let Some(request) = core.finish_batch(sample) {
                let proposal = prepared
                    .optimizer
                    .propose(&request.notes[..request.len], || false)
                    .unwrap();
                let result = RetuneResult::from_targets(request, &proposal.notes);
                assert!(core.apply_result(sample, result, |s, m| output.push((s, m))));
                prepared.optimizer.commit_proposal(proposal);
            }
            core.tick_at_boundary(sample, |s, m| output.push((s, m)));
        }
        core.advance_before(end, |s, m| output.push((s, m)));
    }
    assert!(core.voices().is_empty());
    output
}

#[test]
fn offline_sample_timeline_is_deterministic_and_block_size_independent() {
    let settings = EngineSettings::default();
    let expected = render(32, &settings);
    for block in [32, 64, 128, 256, 512, 1024] {
        assert_eq!(render(block, &settings), expected);
    }
}

#[test]
fn embedded_non_octave_scala_roundtrip_needs_no_files() {
    let settings = EngineSettings { scale: Some(EmbeddedScale {
        name: "tritave".into(), scl: "! embedded\n13ED3\n13\n146.3042308358\n292.6084616716\n438.9126925074\n585.2169233432\n731.521154179\n877.8253850148\n1024.1296158506\n1170.4338466864\n1316.7380775222\n1463.042308358\n1609.3465391938\n1755.6507700296\n3/1\n".into(), kbm: None, root_note: 60,
    }), ..EngineSettings::default() };
    let restored: EngineSettings =
        serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
    assert_eq!(render(64, &settings), render(512, &restored));
}

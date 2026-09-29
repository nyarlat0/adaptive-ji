use adaptive_ji_core::{
    event::MidiEvent,
    realtime::{RealtimeCore, RetuneResult},
    settings::EngineSettings,
    tuning::{ActiveNote, TuningStrategy},
};
use adaptive_ji_runtime::{OptimizerWorker, session::Session};
use std::time::{Duration, Instant};
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
fn speculative_search_preserves_objective_and_only_acknowledged_memory_commits() {
    let settings = EngineSettings::default();
    let mut direct = settings.prepare().unwrap().optimizer;
    let mut speculative = settings.prepare().unwrap().optimizer;
    let first = [
        ActiveNote {
            id: 1,
            input_note: 60,
            current_offset_cents: None,
        },
        ActiveNote {
            id: 2,
            input_note: 64,
            current_offset_cents: None,
        },
        ActiveNote {
            id: 3,
            input_note: 67,
            current_offset_cents: None,
        },
    ];
    assert!(speculative.propose(&first, || true).is_none());
    let dropped = speculative.propose(&first, || false).unwrap();
    let again = speculative.propose(&first, || false).unwrap();
    assert_eq!(dropped.notes, again.notes);
    assert_eq!(direct.tune(&first), again.notes);
    speculative.commit_proposal(again);
    let second = [
        first[1],
        first[2],
        ActiveNote {
            id: 4,
            input_note: 71,
            current_offset_cents: None,
        },
    ];
    assert_eq!(
        direct.tune(&second),
        speculative.propose(&second, || false).unwrap().notes
    );
}

#[test]
fn worker_coalesces_newest_generation_and_does_not_resurrect_released_notes() {
    let p = EngineSettings::default().prepare().unwrap();
    let mut core = RealtimeCore::new(&p, 48000.0, true).unwrap();
    let worker = OptimizerWorker::start(p.optimizer).unwrap();
    for n in 0..30 {
        core.begin_batch();
        core.event(n, cc(123, 0), |_, _| {});
        core.event(n, on(60 + (n % 12) as u8), |_, _| {});
        worker.request(core.finish_batch(n).unwrap());
    }
    let request = core.reset(31, |_, _| {});
    worker.request(request);
    let start = Instant::now();
    loop {
        if let Some(result) = worker.take_result()
            && core.apply_result(32, result, |_, _| panic!("dead voice output"))
        {
            assert_eq!(result.generation, request.generation);
            assert_eq!(result.len, 0);
            worker.acknowledge(result.generation);
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "worker timeout");
        std::thread::yield_now();
    }
    core.advance_before(10000, |_, _| panic!("stale glide"));
}

fn render(block: u32) -> Vec<(u64, [u8; 3])> {
    let mut session = Session::new(&EngineSettings::default(), 48000.0).unwrap();
    let events = [
        (17, on(60)),
        (17, on(64)),
        (17, on(67)),
        (400, cc(64, 127)),
        (500, off(60)),
        (501, on(60)),
        (700, off(60)),
        (800, on(70)),
        (901, cc(64, 0)),
        (2300, cc(123, 0)),
    ];
    let mut output = Vec::new();
    for start in (0..4096u64).step_by(block as usize) {
        let frames = block.min((4096 - start) as u32);
        session.process(
            start,
            frames,
            true,
            events
                .iter()
                .filter(|(s, _)| *s >= start && *s < start + u64::from(frames))
                .map(|(s, m)| ((*s - start) as u32, *m)),
            |s, m| output.push((s, m)),
        );
    }
    assert!(session.core.voices().is_empty());
    output
}
#[test]
fn session_offline_batching_and_glide_independent_of_block_size() {
    let expected = render(32);
    for block in [32, 64, 128, 256, 512, 1024] {
        assert_eq!(expected, render(block));
    }
    assert!(
        expected
            .iter()
            .filter(|(_, m)| m[0] & 0xf0 == 0x90)
            .all(|(s, _)| [17, 501, 800].contains(s))
    );
}

#[test]
fn mapped_sustain_repeated_notes_keep_allocations_and_pedal_up_cancels_glide() {
    let settings = EngineSettings::default();
    let p = settings.prepare().unwrap();
    let mut core = RealtimeCore::new(&p, 48000.0, true).unwrap();
    core.begin_batch();
    for event in [cc(64, 127), on(60), off(60), on(60), off(60), on(64)] {
        core.event(7, event, |_, _| {});
    }
    let request = core.finish_batch(7).unwrap();
    assert_eq!(core.voices().len(), 3);
    let targets: Vec<_> = request.notes[..3]
        .iter()
        .map(|n| adaptive_ji_core::tuning::TunedNote {
            id: n.id,
            ratio: adaptive_ji_core::tuning::Ratio::ONE,
            cents_offset: 13.0,
        })
        .collect();
    assert!(core.apply_result(8, RetuneResult::from_targets(request, &targets), |_, _| {}));
    let mut output = Vec::new();
    core.begin_batch();
    core.event(20, cc(64, 0), |s, m| output.push((s, m)));
    core.finish_batch(20);
    assert_eq!(core.voices().len(), 1);
    assert_eq!(core.voices()[0].input_note, 64);
    core.advance_before(4000, |s, m| output.push((s, m)));
    assert!(
        output
            .iter()
            .filter(|(s, m)| *s > 20 && m[0] & 0xf0 == 0xe0)
            .all(|(_, m)| m[0] & 15 == core.voices()[0].output_channel)
    );
}

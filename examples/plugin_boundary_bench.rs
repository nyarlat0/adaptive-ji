//! Compare redundant retunes, batched search and the nonblocking callback.
use adaptive_ji_core::{
    event::MidiEvent,
    mpe::MpeProcessor,
    realtime::{RealtimeCore, RetuneResult},
    settings::EngineSettings,
};
use adaptive_ji_runtime::session::Session;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box,
    time::{Duration, Instant},
};
struct Count;
thread_local! { static ALLOCS: Cell<Option<usize>> = const {Cell::new(None)}; }
unsafe impl GlobalAlloc for Count {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.with(|c| {
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
        ALLOCS.with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + 1));
            }
        });
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static ALLOCATOR: Count = Count;
fn measured(f: impl FnOnce()) -> (f64, usize) {
    ALLOCS.with(|c| c.set(Some(0)));
    let t = Instant::now();
    f();
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let n = ALLOCS.with(|c| c.replace(None).unwrap());
    (ms, n)
}
fn on(note: u8) -> MidiEvent {
    MidiEvent::NoteOn {
        channel: 0,
        note,
        velocity: 100,
    }
}
fn run(label: &str, settings: &EngineSettings, n: usize) {
    let keys = [48, 49, 51, 52, 54, 55, 57, 58, 60, 61, 63, 64, 66, 67, 70];
    let p = settings.prepare().unwrap();
    let mut legacy = MpeProcessor::with_glide(p.optimizer, p.bend_range, p.glide);
    legacy.set_diagnostics(false);
    let seq = measured(|| {
        for &note in &keys[..n] {
            legacy
                .process_at_to(on(note), Duration::ZERO, |m| {
                    black_box(m);
                    Ok::<_, ()>(())
                })
                .unwrap();
        }
    });
    let mut p = settings.prepare().unwrap();
    let mut core = RealtimeCore::new(&p, 48000.0, true).unwrap();
    core.begin_batch();
    for &note in &keys[..n] {
        core.event(0, on(note), |_, m| {
            black_box(m);
        });
    }
    let request = core.finish_batch(0).unwrap();
    let batch = measured(|| {
        black_box(p.optimizer.propose(&request.notes[..n], || false).unwrap());
    });
    let warm = measured(|| {
        black_box(p.optimizer.propose(&request.notes[..n], || false).unwrap());
    });
    let mut session = Session::new(settings, 48000.0).unwrap();
    let began = Instant::now();
    let rt = measured(|| {
        session.process(
            0,
            64,
            false,
            keys[..n].iter().map(|&k| (7, on(k))),
            |_, m| {
                black_box(m);
            },
        )
    });
    let mut rt_max = rt.0;
    let mut allocations = rt.1;
    let mut sample = 64;
    while session.core.accepted_generation() == 0 && began.elapsed() < Duration::from_secs(30) {
        let (time, count) = measured(|| {
            session.process(sample, 64, false, [], |_, m| {
                black_box(m);
            })
        });
        rt_max = rt_max.max(time);
        allocations += count;
        sample += 64;
        std::thread::yield_now();
    }
    assert!(session.core.accepted_generation() > 0, "worker timeout");
    let turnaround = began.elapsed().as_secs_f64() * 1000.0;
    let mut repeated_max: f64 = 0.0;
    for k in 0..20 {
        let (time, count) = measured(|| {
            session.process(
                sample,
                64,
                false,
                [(
                    0,
                    MidiEvent::ControlChange {
                        channel: 0,
                        controller: 123,
                        value: 0,
                    },
                )]
                .into_iter()
                .chain(keys[..n].iter().map(|&note| (0, on(note + (k % 2) as u8)))),
                |_, m| {
                    black_box(m);
                },
            )
        });
        repeated_max = repeated_max.max(time);
        allocations += count;
        sample += 64;
    }
    assert_eq!(allocations, 0);
    // Fixed result application benchmark, independent of worker scheduling.
    let proposal = p.optimizer.propose(&request.notes[..n], || false).unwrap();
    let result = RetuneResult::from_targets(request, &proposal.notes);
    let apply = measured(|| {
        black_box(core.apply_result(1, result, |_, m| {
            black_box(m);
        }));
        core.advance_before(2000, |_, m| {
            black_box(m);
        });
    });
    println!(
        "{label} voices={n} sequential_cold={:.3}ms batch_cold={:.3}ms warm={:.3}ms optimizer_allocs={} rt_initial={:.4}ms rt_max={rt_max:.4}ms repeat_max={repeated_max:.4}ms apply+glide={:.4}ms rt_allocs={allocations} worker_turnaround={turnaround:.3}ms",
        seq.0, batch.0, warm.0, warm.1, rt.0, apply.0
    );
}
fn main() {
    let settings = EngineSettings::default();
    for n in [3, 6, 10, 15] {
        run("default", &settings, n);
    }
    let mut settings = settings;
    settings.custom_spectrum_toml = Some(
        (1..=128)
            .map(|n| {
                format!(
                    "[[partial]]\nratio={}\namplitude={}\n",
                    f64::from(n) + 0.001 * f64::from(n % 3),
                    1.0 / f64::from(n).sqrt()
                )
            })
            .collect(),
    );
    run("custom128", &settings, 15);
}

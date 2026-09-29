//! Audit the current synchronous engine before putting it in a host callback.
//! cargo run --release --example plugin_realtime_audit -- 100
//! This is a measurement tool, NOT a realtime-safe plugin implementation.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    convert::Infallible,
    hint::black_box,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};

use adaptive_ji::{
    glide::GlideConfig,
    midi::MidiEvent,
    mpe::MpeProcessor,
    tuning::{AdaptiveRationalOptimizer, OptimizerConfig, PrimeLimit},
};

struct CountingAllocator;

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static DEALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static REQUESTED_BYTES: AtomicU64 = AtomicU64::new(0);

fn allocation(size: usize) {
    if COUNTING.load(Ordering::Relaxed) {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        REQUESTED_BYTES.fetch_add(size as u64, Ordering::Relaxed);
    }
}

// SAFETY: every operation delegates to System with the original layout and
// pointer. Instrumentation uses only atomics and never recursively allocates.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        allocation(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        allocation(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        allocation(size);
        unsafe { System.realloc(ptr, layout, size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if COUNTING.load(Ordering::Relaxed) {
            DEALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Clone, Copy)]
struct Measurement {
    elapsed: Duration,
    allocations: u64,
    deallocations: u64,
    requested_bytes: u64,
}

fn measure(mut work: impl FnMut()) -> Measurement {
    ALLOCATIONS.store(0, Ordering::Relaxed);
    DEALLOCATIONS.store(0, Ordering::Relaxed);
    REQUESTED_BYTES.store(0, Ordering::Relaxed);
    let started = Instant::now();
    COUNTING.store(true, Ordering::Relaxed);
    work();
    COUNTING.store(false, Ordering::Relaxed);
    Measurement {
        elapsed: started.elapsed(),
        allocations: ALLOCATIONS.load(Ordering::Relaxed),
        deallocations: DEALLOCATIONS.load(Ordering::Relaxed),
        requested_bytes: REQUESTED_BYTES.load(Ordering::Relaxed),
    }
}

type Engine = MpeProcessor<AdaptiveRationalOptimizer>;

fn engine() -> Engine {
    // Current standalone CLI defaults, which differ from library defaults.
    let config = OptimizerConfig {
        prime_limit: PrimeLimit::new(11).unwrap(),
        max_deviation_cents: 50.0,
        ..OptimizerConfig::default()
    };
    MpeProcessor::with_glide(
        AdaptiveRationalOptimizer::new(config).unwrap(),
        "2".parse().unwrap(),
        GlideConfig::default(),
    )
}

fn note_on(note: u8) -> MidiEvent {
    MidiEvent::NoteOn {
        channel: 0,
        note,
        velocity: 100,
    }
}

fn process(engine: &mut Engine, event: MidiEvent, now: Duration) {
    engine
        .process_at_to(black_box(event), now, |message| {
            black_box(message);
            Ok::<_, Infallible>(())
        })
        .unwrap();
}

fn report(label: &str, measurements: &mut [Measurement]) {
    measurements.sort_by_key(|m| m.elapsed);
    let n = measurements.len();
    let p95 = measurements[(n * 95 / 100).min(n - 1)];
    let max = measurements[n - 1];
    println!(
        "  {label}: median={:.3}ms p95={:.3}ms observed_max={:.3}ms max_alloc/realloc={} max_dealloc={} max_requested_bytes={} (not peak live memory)",
        measurements[n / 2].elapsed.as_secs_f64() * 1000.0,
        p95.elapsed.as_secs_f64() * 1000.0,
        max.elapsed.as_secs_f64() * 1000.0,
        measurements.iter().map(|m| m.allocations).max().unwrap(),
        measurements.iter().map(|m| m.deallocations).max().unwrap(),
        measurements
            .iter()
            .map(|m| m.requested_bytes)
            .max()
            .unwrap(),
    );
}

fn main() {
    let iterations = std::env::args()
        .nth(1)
        .map(|value| value.parse::<usize>().expect("positive iteration count"))
        .unwrap_or(100)
        .max(1);
    println!("Synchronous MIDI->MPE audit, iterations={iterations}, logging disabled");
    println!("11-limit, max deviation=50c, K=8, beam=2048, harmonic 12 partials,");
    println!("floating anchor, relation memory on, glide=35ms; no ALSA/host wrapper");
    println!("48kHz deadlines: 64 frames=1.333ms, 128=2.667ms, 256=5.333ms");
    println!("Observed maxima are NOT worst-case execution-time guarantees.");
    let pitches = [48, 49, 51, 52, 54, 55, 57, 58, 60, 61, 63, 64, 66, 67, 70];
    for count in [3, 6, 10, 15] {
        println!("{count} voices:");
        let mut cold = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let mut engine = engine();
            cold.push(measure(|| {
                // All attacks at one sample offset, as in a piano-roll chord.
                for &note in &pitches[..count] {
                    process(&mut engine, note_on(note), Duration::ZERO);
                }
            }));
        }
        report("cold same-sample chord", &mut cold);

        let mut engine = engine();
        for &note in &pitches[..count] {
            process(&mut engine, note_on(note), Duration::ZERO);
        }
        let last = pitches[count - 1];
        let mut warm = Vec::with_capacity(iterations);
        for i in 0..iterations {
            let now = Duration::from_millis((i as u64 + 1) * 100);
            process(
                &mut engine,
                MidiEvent::NoteOff {
                    channel: 0,
                    note: last,
                    velocity: 0,
                },
                now,
            );
            warm.push(measure(|| process(&mut engine, note_on(last), now)));
        }
        report("warm final NoteOn retune", &mut warm);
    }
}

# Plugin architecture and realtime audit

CLAP and VST3 share `adaptive-ji-runtime::Session` and the same portable core.
The existing standalone CLI keeps its synchronous event policy and musical
defaults. The plugin has **zero declared latency**: NoteOn is never held waiting
for the optimizer. This is not a synthesizer and has no audio ports.

## Execution boundary

- `crates/core`: musical events, Scala/profile text parsing, rational optimizer,
  relation memory, MPE allocator, sustain, panic and glide. Production core has
  no filesystem, ALSA or plugin API dependency.
- `crates/runtime`: fixed-size request/result packets, generation and phrase
  epoch, bounded lock-free queues, one optimizer worker per session, sample clock.
- `crates/plugin`: common CLAP/VST3 frontend, editor, portable state, transport
  tracking and prepared-configuration exchange.
- `src/`: existing standalone CLI, filesystem loading and ALSA/midir transport.

All incoming events at an equal sample offset are applied in original MIDI order
before **one full optimization request** is submitted. The cheap immediate path
retains existing voice targets and starts new notes at prepared Scala/12-TET base
pitch (bend before NoteOn). It does not evaluate roughness or run beam search.
This boundary can accept a future bounded local solver without changing MIDI IO.

The worker receives a single coherent audible-offset snapshot, plus stable voice
IDs, generation and epoch. Queues hold at most two fixed-size packets; new work
replaces queued old work. Cancellation checks occur before search, during pair
precomputation and beam expansion. An individual candidate-generation operation
is not interruptible, but its result cannot bypass generation checks.
The callback rejects outdated/mismatched results and never resurrects a voice.
Accepted results retarget surviving voices from their interpolated position at
the application sample, without new NoteOn. Movement scoring uses the request's
audible snapshot; naturally, asynchronous output may have progressed meanwhile.

Speculative relation memory is committed only after acknowledgement. Abandoned
searches must not become harmonic history. Panic/phrase reset changes the epoch.
Glide ticks do not invoke the optimizer.

## Realtime and offline

Realtime processing performs no search, IO, logging, thread creation, sleeps or
mutex access in the application callback. Voice/FIFO state is bounded to 15
voices plus fixed per-key counters. Glide and event scratch are preallocated.
Requests/results contain no owning heap objects. Old configured sessions are
retired off-thread, including worker join/destruction.

Beam, candidate, unary, spectrum and pair-matrix scratch is reused by the full
optimizer. **The full optimizer is not allocation-free**: candidate-cache misses,
relation fingerprints/memory snapshots and solution construction still allocate,
exclusively on the worker (or in non-realtime offline processing). This distinction
is intentional and measurable; it is not a claim that the beam search became
suitable for an audio callback.

Offline mode bypasses worker scheduling and solves each batch synchronously,
using the same objective. It retains the immediate base attack and then applies
the final solution at that event sample. Sample-based glide deadlines make this
path identical across tested block sizes 32/64/128/256/512/1024. Realtime refinement
arrival is opportunistic and therefore not bit-identical to offline output.

Tests instrument the allocator on the calling thread, not the worker, including
first chord, sustain, repeated voices, glide, panic and the plugin callback.
The framework's own event spool capacities and host event delivery remain a
separate integration boundary: arbitrary oversized host event floods are not
covered by a universal allocation-free guarantee. Do not interpret the tests as
a WCET proof for arbitrary hosts or configurations.

## Measurements

Run:

```sh
cargo run --release --example plugin_boundary_bench
cargo run --release --example plugin_realtime_audit -- 30
```

The second command preserves the old sequential standalone audit for comparison.
The first separately measures sequential searches, one simultaneous batch,
warm optimizer, immediate callback work, repeated changes, worker turnaround,
and result application/glide. Counting is thread-local. Output sinks do not allocate.
Worker turnaround includes queue polling and OS scheduling, not plugin latency.

Example Ryzen 9 5900X release run, 2026-09-29 (single-run diagnostics, not WCET):

| Voices | Sequential cold | One batch cold | Warm search | Initial callback | Repeated callback max | Worker turnaround |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 3 | 0.160 ms | 0.147 ms | 0.096 ms | 2.1 µs | 2.5 µs | 1.007 ms |
| 6 | 3.614 ms | 3.420 ms | 2.357 ms | 4.2 µs | 4.0 µs | 4.180 ms |
| 10 | 19.611 ms | 8.104 ms | 7.279 ms | 6.9 µs | 6.4 µs | 9.596 ms |
| 15 | 71.445 ms | 16.761 ms | 16.410 ms | 10.3 µs | 13.1 µs | 19.902 ms |

Measured realtime allocations: **0**. Applying a 15-voice result and traversing
its glide cost about 6.1 µs. Warm full-search allocations remained 176/1416/4865/
12026 for these cases, outside RT. The old ~73 ms cold measurement was largely
redundant intermediate retuning, not one 15-voice solve.

Stress: a 128-partial fractional custom spectrum at otherwise default settings
took ~769 ms cold / ~806 ms warm for one 15-voice search, ~1.15 s worker turnaround
under that run's scheduling. Initial callback remained ~12 µs, zero allocations.
This is **not musically responsive refinement**; use smaller spectra for live
playing. Maximum beam/K/complexity combinations can be still slower; no finite
general deadline is promised. Neither search quality nor musical defaults are
silently reduced.

## Reset, state and format boundaries

Stop/restart, detected sample-position discontinuity, explicit panic and host
reset invalidate pending work and reuse mapped-output panic. If the host stops
calling process immediately on deactivate, the format supplies no output sink:
cleanup is delivered at the next process opportunity. The host must also terminate
downstream notes when tearing down a graph. A plugin cannot deliver MIDI through
an event sink the host no longer calls.

Configuration is non-automatable, prepared outside process and exchanged at a
block boundary (releasing current voices). Host project-state restore requires
stopped processing; this avoids legacy framework state reinitialization in the
audio callback. GUI configuration uses a separate prepared-session exchange.

Framework source is pinned and locally patched, with source/license/changes in
`vendor/logic_nih_plug/PATCH.md`. In particular the plugin's framework ownership
uses atomic borrow checking, not a callback mutex, and legacy GUI-state rendezvous
is disabled for this frontend. Upstream CLAP state-length validation and host
rescan notification required corrections discovered by the validator.

CLAP and VST3 event-routing support varies by host. A host must support a
note-effect/event output and preserve MPE channels and CC/pitch bends. Format
validator success does not establish end-to-end compatibility with every host.

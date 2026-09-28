# Local midir 0.11.0 patch

This directory vendors the published midir 0.11.0 crate (MIT; see LICENSE).
Cargo.toml's `[patch.crates-io]` selects it for adaptive-ji and its tests.
All upstream source files and manifests are retained (line endings normalized
to LF and final newline added where missing). Registry bookkeeping,
CI configuration and the dependency crate's Cargo.lock are omitted.

Local changes:

- `src/backend/alsa/mod.rs`: replace the unconditional `ev.get_time().unwrap()`
  in the receive thread with `timestamp::event_timestamp_micros`.
- `src/backend/alsa/timestamp.rs`: preserve real-time ALSA timestamps and use
  `Instant::elapsed()` from the input handler for events without one. Tick
  timestamps (including default/direct events) are not interpreted as seconds.
  MIDI bytes are still delivered, including NoteOff and panic CC messages.

The adapter ignores the callback timestamp and uses its own monotonic event-loop
clock for tuning/glide, so the fallback does not change those semantics.
This fixes the reported panic at upstream `alsa/mod.rs:911`. It does not claim
to resolve every possible DAW loop or downstream synth stuck-note issue.

`tests/alsa_timestamp.rs` in the parent project tests this exact helper against
real ALSA Event objects without opening `/dev/snd/seq`. It runs in `cargo test`.
To remove the patch, first verify the same regression tests against an upstream
replacement and ensure its receive path no longer unwraps absent timestamps.

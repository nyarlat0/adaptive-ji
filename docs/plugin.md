# adaptive-ji CLAP / VST3

```text
note/MIDI track -> adaptive-ji plugin -> MPE-capable synthesizer
```

One event input, one event output; no audio input/output. A host must support
note-effect routing and forward member-channel notes, CC and Pitch Bend.
VST3 controllers use the framework's LegacyMidiCCOutEvent output mapping.
No CLAP-only tuning extension is required by the receiving synth.

## Build

On Linux the GUI requires X11/OpenGL development libraries (XWayland under
Wayland). The standalone additionally requires ALSA development libraries.

```sh
cargo run -p xtask -- bundle adaptive-ji-plugin --release
```

Bundles:

- `target/bundled/adaptive-ji-plugin.clap`
- `target/bundled/adaptive-ji-plugin.vst3/`

Copy the desired bundle into your host's configured plugin search directory.
The existing `adaptive-ji --input ...` / `--virtual-input` CLI remains available.
Its synchronous event policy is preserved, whereas the plugin uses asynchronous
refinement to avoid blocking live notes.

## Playing

Enable MPE in the receiving instrument, with member channels 2..16, and set
its pitch-bend range equal to adaptive-ji (default ±2 semitones). RPN sensitivity
is also emitted. Disable/reduce additional synth bend smoothing if using the
default 35 ms adapter glide to avoid double smoothing.

By default, NoteOn attacks immediately at Scala/12-TET base pitch. Simultaneous events make
one optimizer request; the worker can improve tuning shortly afterwards through
glide, without retriggering. In this mode there is **no declared latency and no delayed NoteOn
queue**. The initial fast path keeps existing targets and assigns base pitches
to new voices; it is not an additional approximate beam search.

The editor's **Note attack / Атака ноты** selector also offers
**Сначала подобрать строй (ждать JI)** (`attack_mode: "tuned"`). In this mode,
new voices reserve their channels but remain silent until a valid optimizer
result arrives. They receive their final target Pitch Bend **before NoteOn**,
without an initial base-pitch attack or attack glide. Already sounding voices
still glide to updated targets normally. Select the mode, then **Validate and
apply**; the setting is saved in the project. Old projects without this field
retain `immediate` behavior.

Waiting mode restores the *tuned attack* policy, not synchronous audio-thread
search: it deliberately introduces variable attack delay (search/queue time,
plus up to a processing block). It does not declare a fixed host latency or
time-shift later NoteOffs. Short notes released before the result arrives are
cancelled unless sustained. Pending sustained voices attack at the solved pitch
then receive NoteOff under the pedal; pedal-up/reset cancels those still pending.
Continuous changes can keep superseding a slow search and postpone attacks.
Standalone is unchanged. Batching/asynchronous result timing means this is not
a promise of bit-identical output to the old sequential standalone performance.

Offline rendering runs the full solver synchronously once per sample-offset
batch. It is deterministic across block sizes, but is not an exact replay of
the wall-clock worker completion times heard during realtime playback. In tuned
attack mode, offline NoteOn starts at the input sample with the solved pitch.

## Editor and project state

Open the custom editor. Expand configuration groups, edit values, then press
the green **Validate and apply** button in the top toolbar, next to **Reload
current settings**. **Reset to defaults** in the same toolbar restores the draft's
default settings and clears imported Scala/custom spectrum data and import paths;
press **Validate and apply** to activate those defaults. Applying releases current notes and swaps a prepared
configuration at a processing-block boundary. Parameters are deliberately
non-automatable configuration; the host's configuration-revision parameter
marks editor changes, not a musical automation lane.

The **Live notes / Звучащие ноты** panel uses 22 px rows and shows input note,
voice ID, held/pedal/waiting status, chosen rational relation, current interpolated
and target cents, mapped output note and MPE channel. Ratios refer to the displayed
reference of the last accepted solution; that reference is not necessarily a
tonic. An em dash means no solution/current sounding pitch yet. During a search,
the last accepted targets remain displayed with an updating indicator. Cents
include Scala's base displacement relative to the *input* MIDI note's 12-TET
pitch, not just the smaller bend around its remapped output note. Live data is
not saved in project state. Fixed-size lock-free snapshots keep formatting and
painting entirely outside the audio callback.

The editor exposes:

- `optimizer`: prime limit, complexity/deviation, candidate count, beam width;
- `weights`: harmonic, deviation, movement, roughness and memory;
- anchor mode (`Floating`/`Fixed`) and maximum cents;
- spectrum (`Harmonic`, `Sine`, `Saw`, `Square`, `Soft`, `Bright`), partial count
  and rolloff;
- glide duration/tick interval in milliseconds and bend range;
- memory mode (`Off`/`Relations`), decay, weight and reset-on-empty.

Values use the same validations/defaults as standalone. Unsupported values
produce an editor error instead of replacing the working configuration.

Enter an arbitrary `.scl` path and optional `.kbm` path, then **Import Scala into
draft** and **Validate and apply**. With no KBM, edit `root_note` (default 60).
There is no filename-specific scale logic. **Clear scale / 12-TET base** reverts
the draft. Import custom spectrum TOML in the same way; **Use built-in spectrum**
clears it. The current draft scale's name is displayed. **Reload current settings**
refreshes the editor after a host project-state restore.

Scala supplies base pitch geometry, including non-octave periods. The adaptive
optimizer remains active on top. Scale, KBM and custom spectrum contents—not
just file paths—are embedded in serialized project state. The same state is used
by both formats. Moving source files does not change restored sound. State blobs
are limited to 16 MiB. Held voices, sustain, live glide and phrase memory are not
serialized. Restoring host state requires stopped processing.

**PANIC** schedules the shared mapped-NoteOff / sustain-off / member-channel panic
at the next processing opportunity. Stop/restart and detected seek/loop jumps
also reset. Hosts must release downstream voices on graph teardown when they no
longer provide this plugin an output-event callback.

## Performance and validation

See [the realtime audit](plugin-readiness.md) for measured callback/worker times,
allocation boundaries, overload limits and the costly 128-partial stress case.
Large searches stay on the worker; they can take hundreds of milliseconds. In
immediate mode this delays refinement, not attack. In tuned attack mode this
also delays new NoteOns; use the default immediate mode for latency-sensitive playing.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
cargo run --release --example plugin_boundary_bench
clap-validator validate target/bundled/adaptive-ji-plugin.clap
validator target/bundled/adaptive-ji-plugin.vst3
```

Source licensing and dependency notices: [THIRD_PARTY.md](../THIRD_PARTY.md).

Validation on Linux, 2026-09-29: clap-validator 0.4.1 reported 36 passed,
0 failed, 8 skipped; Steinberg VST3 validator (SDK 3.8.1) reported 47 passed,
0 failed. Hardware ALSA loopback tests require `/dev/snd/seq` and are ignored
in the hardware-free test run. Interactive host/GUI and downstream-synth testing
is still necessary before relying on the plugin for a live performance.

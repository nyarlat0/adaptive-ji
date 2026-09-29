# Logic-NIH-plug

[![Builds](https://github.com/LogicCuteGuy/logic_nih_plug/actions/workflows/build.yml/badge.svg?branch=main)](https://github.com/LogicCuteGuy/logic_nih_plug/actions/workflows/build.yml?query=branch%3Amain)
[![Docs](https://github.com/LogicCuteGuy/logic_nih_plug/actions/workflows/docs.yml/badge.svg?branch=main)](https://LogicCuteGuy.github.io/logic_nih_plug/)

**This is a fork of [robbert-vdh/nih-plug](https://github.com/robbert-vdh/nih-plug)**
that adds pure-Rust ports of several JUCE modules. See
[docs/README.md](docs/README.md) for the documentation index, and
[docs/git-workflow.md](docs/git-workflow.md) for fork-specific git/CI notes.

**Upstream NOTE:** `NIH-plug` the plugin framework is currently in maintenance
mode. If you are interested in the framework rather than this fork, please
check out [this community fork](https://codeberg.org/BillyDM/nih-plug).

NIH-plug is an API-agnostic audio plugin framework written in Rust, as well as a
small collection of plugins. The idea is to have a stateful yet simple plugin
API that gets rid of as much unnecessary ceremony wherever possible, while also
keeping the amount of magic to minimum and making it easy to experiment with
different approaches to things. See the [current features](#current-features)
section for more information on the project's current status.

Check out the [documentation](https://nih-plug.robbertvanderhelm.nl/), the
fork docs at [docs/README.md](docs/README.md), or use the
the [cookiecutter template](https://github.com/robbert-vdh/nih-plug-template) to
quickly get started with NIH-plug.

### Table of contents

- [Plugins](#plugins)
- [Framework](#framework)
  - [Current features](#current-features)
  - [Building](#building)
  - [Plugin formats](#plugin-formats)
  - [Example plugins](#example-plugins)
- [JUCE Ported Modules](#juce-ported-modules)
  - [Available Modules](#available-modules)
  - [Documentation](#documentation)
- [Project Structure](#project-structure)
- [Licensing](#licensing)

## Plugins

Check each plugin's readme file for more details on what the plugin actually
does. You can download the development binaries for Linux, Windows and macOS
from the [automated
builds](https://github.com/robbert-vdh/nih-plug/actions/workflows/build.yml?query=branch%3Amaster)
page. Or if you're not signed in on GitHub, then you can also find the latest
nightly build
[here](https://nightly.link/robbert-vdh/nih-plug/workflows/build/master). You
may need to [disable Gatekeeper](https://disable-gatekeeper.github.io/) on macOS to be able to use
the plugins.

Scroll down for more information on the underlying plugin framework.

- [**Buffr Glitch**](plugins/buffr_glitch) is the plugin for you if you enjoy
  the sound of a CD player skipping This plugin is essentially a MIDI triggered
  buffer repeat plugin. When you play a note, the plugin will sample the period
  corresponding to that note's frequency and use that as a single waveform
  cycle. This can end up sounding like an in-tune glitch when used sparingly, or
  like a weird synthesizer when used less subtly.
- [**Crisp**](plugins/crisp) adds a bright crispy top end to any low bass sound.
  Inspired by Polarity's [Fake Distortion](https://youtu.be/MKfFn4L1zeg) video.
- [**Crossover**](plugins/crossover) is as boring as it sounds. It cleanly
  splits the signal into two to five bands using a variety of algorithms. Those
  bands are then sent to auxiliary outputs so they can be accessed and processed
  individually. Meant as an alternative to Bitwig's Multiband FX devices but
  with cleaner crossovers and a linear-phase option.
- [**Diopser**](plugins/diopser) is a totally original phase rotation plugin.
  Useful for oomphing up kickdrums and basses, transforming synths into their
  evil phase-y cousin, and making everything sound like a cheap Sci-Fi laser
  beam.
- [**Loudness War Winner**](plugins/loudness_war_winner) does what it says on
  the tin. Have you ever wanted to show off your dominance by winning the
  loudness war? Neither have I. Dissatisfaction guaranteed.
- [**Puberty Simulator**](plugins/puberty_simulator) is that patent pending One
  Weird Plugin that simulates the male voice change during puberty! If it was
  not already obvious from that sentence, this plugin is a joke, but it might
  actually be useful (or at least interesting) in some situations. This plugin
  pitches the signal down an octave, but it also has the side effect of causing
  things to sound like a cracking voice or to make them sound slightly out of
  tune.
- [**Safety Limiter**](plugins/safety_limiter) is a simple tool to prevent ear
  damage. As soon as there is a peak above 0 dBFS or the specified threshold,
  the plugin will cut over to playing SOS in Morse code, gradually fading out
  again when the input returns back to safe levels. Made for personal use during
  plugin development and intense sound design sessions, but maybe you'll find it
  useful too!
- [**Soft Vacuum**](plugins/soft_vacuum) is a straightforward port of
  Airwindows' [Hard Vacuum](https://www.airwindows.com/hard-vacuum-vst/) plugin
  with parameter smoothing and up to 16x linear-phase oversampling, because I
  liked the distortion and just wished it had oversampling. All credit goes to
  Chris from Airwindows. I just wanted to share this in case anyone else finds
  it useful.
- [**Spectral Compressor**](plugins/spectral_compressor) can squash anything
  into pink noise, apply simultaneous upwards and downwards compressor to
  dynamically match the sidechain signal's spectrum and morph one sound into
  another, and lots more. Have you ever wondered what a 16384 band OTT would
  sound like? Neither have I.

## Framework

### Current features

- Supports both VST3 and [CLAP](https://github.com/free-audio/clap) by simply
  adding the corresponding `nih_export_<api>!(Foo)` macro to your plugin's
  library.
- Standalone binaries can be made by calling `nih_export_standalone(Foo)` from
  your `main()` function. Standalones come with a CLI for configuration and full
  JACK audio, MIDI, and transport support.
- Rich declarative parameter system without any boilerplate.
  - Define parameters for your plugin by adding `FloatParam`, `IntParam`,
    `BoolParam`, and `EnumParam<T>` fields to your parameter struct, assign
    stable IDs to them with the `#[id = "foobar"]`, and a `#[derive(Params)]`
    does all of the boring work for you.
  - Parameters can have complex value distributions and the parameter objects
    come with built-in smoothers and callbacks.
  - Use simple enums deriving the `Enum` trait with the `EnumParam<T>` parameter
    type for parameters that allow the user to choose between multiple discrete
    options. That way you can use regular Rust pattern matching when working
    with these values without having to do any conversions yourself.
  - Store additional non-parameter state for your plugin by adding any field
    that can be serialized with [Serde](https://serde.rs/) to your plugin's
    `Params` object and annotating them with `#[persist = "key"]`.
  - Optional support for state migrations, for handling breaking changes in
    plugin parameters.
  - Group your parameters into logical groups by nesting `Params` objects using
    the `#[nested(group = "...")]`attribute.
  - The `#[nested]` attribute also enables you to use multiple copies of the
    same parameter, either as regular object fields or through arrays.
  - When needed, you can also provide your own implementation for the `Params`
    trait to enable compile time generated parameters and other bespoke
    functionality.
- Stateful. Behaves mostly like JUCE, just without all of the boilerplate.
- Comes with a simple yet powerful way to asynchronously run background tasks
  from a plugin that's both type-safe and realtime-safe.
- Does not make any assumptions on how you want to process audio, but does come
  with utilities and adapters to help with common access patterns.
  - Efficiently iterate over an audio buffer either per-sample per-channel,
    per-block per-channel, or even per-block per-sample-per-channel with the
    option to manually index the buffer or get access to a channel slice at any
    time.
  - Easily leverage per-channel SIMD using the SIMD adapters on the buffer and
    block iterators.
  - Comes with bring-your-own-FFT adapters for common (inverse) short-time
    Fourier Transform operations. More to come.
- Optional sample accurate automation support for VST3 and CLAP that can be
  enabled by setting the `Plugin::SAMPLE_ACCURATE_AUTOMATION` constant to
  `true`.
- Optional support for compressing the human readable JSON state files using
  [Zstandard](https://en.wikipedia.org/wiki/Zstd).
- Comes with adapters for popular Rust GUI frameworks as well as some basic
  widgets for them that integrate with NIH-plug's parameter system. Currently
  there's support for [egui](logic_nih_plug_egui), [iced](logic_nih_plug_iced) and
  [VIZIA](logic_nih_plug_vizia).
  - A simple and safe API for state saving and restoring from the editor is
    provided by the framework if you want to do your own internal preset
    management.
- Full support for receiving and outputting both modern polyphonic note
  expression events as well as MIDI CCs, channel pressure, and pitch bend for
  CLAP and VST3.
  - MIDI SysEx is also supported. Plugins can define their own structs or sum
    types to wrap around those messages so they don't need to interact with raw
    byte buffers in the process function.
- Support for flexible dynamic buffer configurations, including variable numbers
  of input and output ports.
- First-class support several more exotic CLAP features:
  - Both monophonic and polyphonic parameter modulation are supported.
  - Plugins can declaratively define pages of remote controls that DAWs can bind
    to hardware controllers.
- A plugin bundler accessible through the
  `cargo xtask bundle <package> <build_arguments>` command that automatically
  detects which plugin targets your plugin exposes and creates the correct
  plugin bundles for your target operating system and architecture, with
  cross-compilation support. The cargo subcommand can easily be added to [your
  own project](https://github.com/robbert-vdh/nih-plug/tree/master/logic_nih_plug_xtask)
  as an alias or [globally](https://github.com/robbert-vdh/nih-plug/tree/master/cargo_logic_nih_plug)
  as a regular cargo subcommand.
- Tested on Linux and Windows, with limited testing on macOS. Windows support
  has mostly been tested through Wine with
  [yabridge](https://github.com/robbert-vdh/yabridge).
- See the [`Plugin`](src/plugin.rs) trait's documentation for an incomplete list
  of the functionality that has currently not yet been implemented.

### Building

NIH-plug works with the latest stable Rust compiler.

After installing [Rust](https://rustup.rs/), you can compile any of the plugins
in the `plugins` directory in the following way, replacing `gain` with the name
of the plugin:

```shell
cargo xtask bundle gain --release
```

### Plugin formats

NIH-plug can export plugins in multiple formats:

- **VST3** - Steinberg's modern plugin format (built-in)
- **CLAP** - Free Audio's modern plugin format (built-in)
- **VST2** - Legacy VST format (optional, deprecated)
- **AU** - Apple's Audio Units for macOS (optional)
- **AUv3** - Modern Audio Units for iOS/macOS (optional)
- **LV2** - Open-source format for Linux (optional)
- **AAX** - Avid's format for Pro Tools (optional, requires SDK)

Exporting a specific plugin format for a plugin is as simple as calling the
`nih_export_<format>!(Foo);` macro. The `cargo xtask bundle` command will detect
which plugin formats your plugin supports and create the appropriate bundles
accordingly, even when cross compiling.

For detailed information on multi-format examples, see the
[Multi-Format Plugin Examples](plugins/examples/FORMAT_EXAMPLES.md).

### Example plugins

The best way to get an idea for what the API looks like is to look at the
examples.

- [**gain**](plugins/examples/gain) — simple smoothed gain plugin with
  serializable state.
- **gain-gui** — gain with a GUI and peak meter, in three flavors:
  [egui](plugins/examples/gain_gui_egui),
  [iced](plugins/examples/gain_gui_iced),
  [VIZIA](plugins/examples/gain_gui_vizia).
  Also [OpenGL](plugins/examples/byo_gui_gl),
  [wgpu](plugins/examples/byo_gui_wgpu),
  [softbuffer](plugins/examples/byo_gui_softbuffer) BYO-GUI examples.
- [**midi_inverter**](plugins/examples/midi_inverter) — note/MIDI event
  transformation.
- [**poly_mod_synth**](plugins/examples/poly_mod_synth) — polyphonic synth with
  CLAP poly modulation.
- **[JUCE-style Examples Portfolio](examples/README.md)** — 22 Rust crates
  porting JUCE `examples/` to NIH-plug. See the full catalog below.
- [**sine**](plugins/examples/sine) — test tone generator with frequency
  smoothing and MIDI input.
- [**stft**](plugins/examples/stft) — short-time Fourier transform overlap-add
  processing.
- [**sysex**](plugins/examples/sysex) — custom SysEx message types.

#### JUCE DSP Plugin Examples

Located in `plugins/examples/dsp/`, these are direct ports of JUCE's DSP demo plugins:

| Example | What it demonstrates |
|---|---|
| `juce_distortion_demo` | Waveshaper distortion |
| `juce_oscillator_demo` | Oscillator waveforms |
| `juce_iir_filter_demo` | IIR filter types |
| `juce_phaser_demo` | Phaser effect |
| `juce_chorus_demo` | Chorus effect |
| `juce_convolution_demo` | Convolution / IR loading |
| `juce_noise_gate_demo` | Noise gate dynamics |
| `juce_limiter_demo` | Limiter dynamics |

#### Standalone Audio Apps

Located in `examples/Audio/` — runnable binaries (`[[bin]]` targets):

| Example | What it demonstrates |
|---|---|
| `audio_playback_demo` | WAV playback via `MockAudioIODevice` |
| `audio_recording_demo` | WAV recording |
| `audio_workgroup_demo` | Two-node shared audio buffer |

#### Utilities

Located in `examples/Utilities/` — file-IO and OSC demos:

| Example | What it demonstrates |
|---|---|
| `wav_reader` | Print WAV header summary |
| `wav_writer` | Write a 1-second sine WAV |
| `midi_file_inspector` | Print SMF tempo, tracks, events |
| `osc_sender_demo` | Send OSC bundles |
| `osc_receiver_demo` | Receive OSC messages |

## JUCE Ported Modules

NIH-plug includes pure Rust implementations of several JUCE modules, providing
DSP algorithms, audio file I/O, data structures, graphics, GUI components, and more
without any C++ dependencies. These modules are designed to integrate seamlessly
with nih-plug while maintaining compatibility with JUCE's design patterns.

### Available Modules

| Module | Description | Features |
|---|---|---|
| `logic_nih_plug_dsp` | Digital signal processing: filters, oscillators, envelopes, convolution, dynamics, reverb, delay, modulation, mixer, analysis (FFT, STFT, level metering), pitch (Phase Vocoder, pitch shift, time stretching), resampling (windowed sinc, Catmull-Rom, Lagrange) | `filters`+`oscillators` (default), `dynamics`/`reverb`/`delay`/`modulation`/`mixer`/`resampling` under `processors`, `analysis`, `pitch`, `full` |
| `logic_nih_plug_audio_formats` | Audio file I/O: WAV, AIFF (+ optional FLAC/OGG), MIDI file read/write with tempo-aware transport (`MidiFilePlayer`) | `midi` (default off) |
| `logic_nih_plug_audio_basics` | Core audio types: `AudioSampleBuffer`, `AudioChannelSet`, `MidiMessage`, `MidiRPN`, `MidiClock`, MTC timecode types | — |
| `logic_nih_plug_audio_devices` | Host-side audio device management: `AudioDeviceManager`, `AudioIODevice` trait, `MockAudioIODevice`. Driver bindings (cpal, coreaudio-rs, ASIO) plug in by implementing the trait | `manager` (default), `full` |
| `logic_nih_plug_audio_processors` | Plugin discovery/management: `PluginDescription`, `PluginFormat` trait, `KnownPluginList`, `PluginDirectoryScanner` | `scanner`, `full` |
| `logic_nih_plug_data` | Data structures: `ValueTree`, `UndoManager`, `CachedValue<T>` | — |
| `logic_nih_plug_gui` | JUCE-style `Component`/`Button`/FlexBox port, BYO-GUI helpers, extra controls (`ComboBox`, `TextEditor`, `MidiKeyboardComponent`), CSS Grid layout, OpenGL support | `components`, `layout`, `graphics`, `text`, `softbuffer-editor`, `gl-editor`, `full` |
| `logic_nih_plug_graphics` | 2D vector graphics (tiny-skia backed `Painter`/`Path`/`Stroke`), glyph arrangement, image rescale/convolve | — |
| `logic_nih_plug_animation` | Easing functions, animation chaining | — |
| `logic_nih_plug_osc` | Open Sound Control: `OscSender`, `OscReceiver`, `OSCArgument`, `OSCBundle` | — |
| `logic_nih_plug_crypto` | Cryptography: SHA, MD5, RSA (textbook) | — |
| `logic_nih_plug_product_unlocking` | JUCE-style licensing: keyfile generation, `OnlineUnlockStatus` state machine, machine ID helpers | `key_generation`, `online_unlock_status`, `full` |
| `logic_nih_plug_video` | Video playback: `VideoFrame` (RGBA8888), `VideoDecoder` (ffmpeg), `VideoComponent` (GUI) | `decoder`, `gui`, `full` |
| `logic_nih_plug_midi_ci` | MIDI 2.0 Capability Inquiry: 32 message types, transport-agnostic `Device` + `DeviceListener` pattern | — |
| `logic_nih_plug_derive` | `#[derive(Params)]` proc macro | — |
| `logic_nih_plug_xtask` | Plugin bundling library (`cargo xtask bundle`) | — |
| `logic_nih_plug_egui` / `_iced` / `_vizia` | GUI backend adapters for egui, iced, and VIZIA | — |

### Documentation

- 📚 [Documentation Index](docs/README.md) — complete docs guide and file inventory
- 🚀 [Getting Started](docs/getting-started.md) — build, test, toolchain, hard rules
- 📖 [DSP & GUI Guide](docs/dsp-and-gui.md) — `logic_nih_plug_dsp`, GUI backends, BYO-GUI
- 📋 [Plugin Guide](docs/plugins.md) — the 9 real plugins + examples directory
- 🔄 [Plugin Skeleton](docs/plugin-skeleton.md) — minimal plugin walkthrough
- 📦 [Bundling Guide](docs/bundling.md) — `cargo xtask`, formats, CI
- 🔀 [JUCE Feature Backlog](docs/TODO.md) — per-crate JUCE feature-parity status
- 💻 Full API docs: `cargo doc --open --workspace`

### Quick Example

```rust
use logic_nih_plug_dsp::filters::IIRFilter;

let mut filter = IIRFilter::new();
filter.set_coefficients(&[1.0, 0.5], &[1.0, -0.5])?;
filter.process(&input, &mut output);
```

## Project Structure

```
logic_nih_plug/
├── src/                          # Core framework (Plugin trait, Buffer, Params, wrappers)
├── logic_nih_plug_derive/        # #[derive(Params)] proc macro
├── logic_nih_plug_dsp/           # DSP algorithms (filters, oscillators, dynamics, …)
├── logic_nih_plug_gui/           # JUCE-style GUI components + layout + OpenGL
├── logic_nih_plug_graphics/      # 2D vector graphics (tiny-skia backed)
├── logic_nih_plug_animation/     # Easing and animation
├── logic_nih_plug_audio_formats/ # WAV/AIFF/FLAC/OGG + MIDI file I/O
├── logic_nih_plug_audio_basics/  # Core audio types (Buffer, MidiMessage, MTC)
├── logic_nih_plug_audio_devices/ # AudioDeviceManager + IODevice trait
├── logic_nih_plug_audio_processors/ # Plugin discovery and scanning
├── logic_nih_plug_data/          # ValueTree, UndoManager, CachedValue
├── logic_nih_plug_osc/           # Open Sound Control
├── logic_nih_plug_crypto/        # SHA, MD5, RSA
├── logic_nih_plug_video/         # Video playback (ffmpeg)
├── logic_nih_plug_midi_ci/       # MIDI 2.0 Capability Inquiry
├── logic_nih_plug_product_unlocking/ # Licensing / keyfile system
├── logic_nih_plug_egui/          # egui GUI backend
├── logic_nih_plug_iced/          # iced GUI backend
├── logic_nih_plug_vizia/         # VIZIA GUI backend
├── logic_nih_plug_xtask/         # Plugin bundling library
├── plugins/                      # Real plugins + example plugins
│   ├── examples/                 # Example plugins (gain, sine, stft, …)
│   │   └── dsp/                  # JUCE DSP demo ports
│   ├── buffr_glitch/             # Real plugins
│   ├── crisp/
│   ├── crossover/
│   ├── diopser/
│   ├── loudness_war_winner/
│   ├── puberty_simulator/
│   ├── safety_limiter/
│   ├── soft_vacuum/
│   └── spectral_compressor/
├── examples/                     # Standalone apps + utilities
│   ├── Audio/                    # Playback, recording, workgroup demos
│   ├── Utilities/                # WAV/MIDI/OSC CLI tools
│   ├── Plugins/                  # Plugin host (CLI + egui GUI)
│   ├── DemoRunner/               # Multi-backend GUI showcase
│   ├── audio-assets/             # Reference WAV fixtures
│   └── midi-assets/              # Reference MIDI fixtures
├── docs/                         # Project documentation
├── specs/                        # Feature specs and design docs
├── xtask/                        # Cargo subcommand shim
└── Cargo.toml                    # Workspace root
```

## Licensing

The framework, its libraries, and the example plugins in `plugins/examples/` are
all licensed under the [ISC license](https://www.isc.org/licenses/). However,
the [VST3 bindings](https://github.com/RustAudio/vst3-sys) used by
`nih_export_vst3!()` are licensed under the GPLv3 license. This means that
unless you replace these bindings with your own bindings made from scratch, any
VST3 plugins built with NIH-plug need to be able to comply with the terms of the
GPLv3 license.

The other plugins in the `plugins/` directory may be licensed under the GPLv3
license. Check the plugin's `Cargo.toml` file for more information.

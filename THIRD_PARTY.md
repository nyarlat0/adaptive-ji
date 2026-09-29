# Third-party code

Original adaptive-ji source is released under [The Unlicense](UNLICENSE).
This does not relicense third-party code, vendored code or dependencies.

- `vendor/midir`: MIT (see its original notices).
- `vendor/logic_nih_plug` and `vendor/logic_nih_plug_egui`: ISC;
  source revision and local changes are recorded in their `PATCH.md` files.
- The pinned framework's `vst3-sys` bindings declare GPL-3.0; the default
  dual-format plugin binary includes these bindings (including its CLAP copy).
  The project's Unlicense does not remove the
  third-party obligations of distributing this combined binary.
  See [the binding manifest](https://github.com/robbert-vdh/vst3-sys/blob/b3ff4d775940f5b476b9d1cca02a90e07e1922a2/Cargo.toml)
  (the exact resolved revision is recorded in Cargo.lock).
- Other dependency licenses are specified by their manifests in the dependency
  graph. Keep their notices when redistributing.

The generic core does not depend on plugin APIs or VST3 bindings.

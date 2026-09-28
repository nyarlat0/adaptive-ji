use std::{path::Path, process::Command};

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_adaptive-ji"))
        .args(args)
        .output()
        .expect("adaptive-ji binary starts")
}

#[test]
fn auto_kbm_and_root_override_are_visible_without_alsa() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("scales/lim_13_operator.scl");
    let path = path.to_str().unwrap();
    let automatic = run(&["--scale", path, "--scale-info"]);
    assert!(automatic.status.success());
    let text = String::from_utf8(automatic.stdout).unwrap();
    assert!(text.contains("lim_13_operator.kbm"));
    assert!(text.contains(" 66 ->  66   -48.671c"));

    let root = run(&["--scale", path, "--root-note", "D4", "--scale-info"]);
    assert!(root.status.success());
    let text = String::from_utf8(root.stdout).unwrap();
    assert!(text.contains("KBM default, root=62"));
    assert!(text.contains(" 62 ->  62    +0.000c"));

    let no_kbm = run(&["--scale", path, "--no-kbm", "--scale-info"]);
    assert!(no_kbm.status.success());
    assert!(
        String::from_utf8(no_kbm.stdout)
            .unwrap()
            .contains("KBM default, root=60")
    );

    let explicit = Path::new(path).with_extension("kbm");
    let explicit = run(&[
        "--scale",
        path,
        "--kbm",
        explicit.to_str().unwrap(),
        "--scale-info",
    ]);
    assert!(explicit.status.success());
}

#[test]
fn invalid_kbm_reports_file_error_before_opening_midi() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("scales/albionbyz.scl");
    let output = run(&[
        "--scale",
        path.to_str().unwrap(),
        "--kbm",
        "/does-not-exist/invalid.kbm",
        "--scale-info",
    ]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("cannot read mapping")
    );
}

#[test]
fn tritave_diagnostics_identify_full_map_edges_and_show_rejected_pitches() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("scales/13_equal_div_3_1.scl");
    let kbm = path.with_extension("kbm");
    let output = run(&[
        "--scale",
        path.to_str().unwrap(),
        "--kbm",
        kbm.to_str().unwrap(),
        "--scale-info",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stdout.contains("Playable input MIDI: 18..106"));
    assert!(stdout.contains("Unavailable input MIDI: 0..17, 107..127"));
    assert!(stdout.contains("  0 -> unavailable  requested MIDI pitch=-27.783"));
    assert!(stdout.contains("107 -> unavailable"));
    assert!(stdout.contains("127 -> unavailable  requested MIDI pitch=158.024"));
    assert!(stdout.contains(" 60 ->  60"));
    assert!(stderr.contains("39 of the full 128-key map, not the connected keyboard"));
    assert!(!stderr.contains("increase --bend-range"));
}

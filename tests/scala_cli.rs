use std::{path::Path, process::Command};

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_adaptive-ji"))
        .args(args)
        .output()
        .expect("adaptive-ji binary starts")
}

#[test]
fn auto_kbm_and_root_override_are_visible_without_alsa() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scales/13-limit scale from Brendan Byrnes Operator.scl");
    let path = path.to_str().unwrap();
    let automatic = run(&["--scale", path, "--scale-info"]);
    assert!(automatic.status.success());
    let text = String::from_utf8(automatic.stdout).unwrap();
    assert!(text.contains("Operator.kbm"));
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

use std::process::Command;

#[test]
fn cli_exposes_profile_options_and_reports_missing_file_before_midi_open() {
    let binary = env!("CARGO_BIN_EXE_adaptive-ji");
    let help = Command::new(binary).arg("--help").output().unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for option in ["--spectrum", "--spectrum-file", "--roughness-partials"] {
        assert!(help.contains(option));
    }
    let invalid_limit = Command::new(binary)
        .args(["--input", "unused", "--prime-limit", "13"])
        .output()
        .unwrap();
    assert!(!invalid_limit.status.success());
    assert!(
        String::from_utf8(invalid_limit.stderr)
            .unwrap()
            .contains("invalid value")
    );
    let missing = Command::new(binary)
        .args([
            "--input",
            "unused",
            "--spectrum-file",
            "/path/that/does/not/exist/spectrum.toml",
        ])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    let error = String::from_utf8(missing.stderr).unwrap();
    assert!(error.contains("cannot read spectrum file"), "{error}");
}

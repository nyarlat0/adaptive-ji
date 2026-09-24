//! Opt-in transport test. Creates isolated virtual ALSA ports and runs the actual binary.
#![cfg(target_os = "linux")]

use std::{
    collections::HashSet,
    process::{Child, Command},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

use adaptive_ji::pitch_bend::{cents_to_pitch_bend, encode_pitch_bend};
use midir::{Ignore, MidiInput, MidiOutput, os::unix::VirtualOutput};

struct RunningApp(Child);

impl Drop for RunningApp {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn receive_through(receiver: &Receiver<Vec<u8>>, last: [u8; 3]) -> Vec<Vec<u8>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut received = Vec::new();
    loop {
        let message = receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap_or_else(|error| panic!("Waiting for {last:?}: {error}; received {received:?}"));
        let done = message == last;
        received.push(message);
        if done {
            return received;
        }
    }
}

fn receive_count(receiver: &Receiver<Vec<u8>>, count: usize) -> Vec<Vec<u8>> {
    let mut messages = Vec::with_capacity(count);
    for _ in 0..count {
        messages.push(
            receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("missing MIDI output"),
        );
    }
    messages
}

fn last_bend(messages: &[Vec<u8>], channel: u8) -> u16 {
    let message = messages
        .iter()
        .rev()
        .find(|message| message[0] == 0xe0 | channel)
        .expect("missing member-channel bend");
    u16::from(message[1]) | (u16::from(message[2]) << 7)
}

#[test]
#[ignore = "requires /dev/snd/seq; run cargo test --test alsa_loopback -- --ignored"]
fn actual_binary_routes_retunes_and_cleans_up_on_sigint() {
    // Sequential runs keep the existing baseline regression and exercise the new default.
    run_strategy("five-limit");
    run_strategy("adaptive-fixed");
    run_strategy("adaptive");
    run_strategy("adaptive-zero");
}

fn run_strategy(strategy: &str) {
    let source_name = format!(
        "adaptive-ji test keyboard {strategy} {}",
        std::process::id()
    );
    let mut keyboard = MidiOutput::new(&source_name)
        .unwrap()
        .create_virtual(&source_name)
        .unwrap();
    let mut monitor = MidiInput::new("adaptive-ji test monitor").unwrap();
    monitor.ignore(Ignore::None);
    let existing: HashSet<_> = monitor
        .ports()
        .iter()
        .map(|port| monitor.port_name(port).unwrap())
        .collect();
    let mut app = RunningApp(
        Command::new(env!("CARGO_BIN_EXE_adaptive-ji"))
            .args(["--input", &source_name, "--bend-range", "2"])
            // Omit the flag for adaptive to check the CLI default as well.
            .args(match strategy {
                "five-limit" => vec!["--strategy", "five-limit"],
                "adaptive-fixed" => vec!["--anchor-mode", "fixed"],
                "adaptive-zero" => vec!["--w-roughness", "0"],
                _ => vec![],
            })
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let output_port = loop {
        let port = monitor.ports().into_iter().find(|port| {
            let name = monitor.port_name(port).unwrap();
            name.contains("Adaptive JI Out") && !existing.contains(&name)
        });
        if let Some(port) = port {
            break port;
        }
        assert!(
            app.0.try_wait().unwrap().is_none(),
            "adaptive-ji exited during startup"
        );
        assert!(
            Instant::now() < deadline,
            "Virtual output port did not appear"
        );
        thread::sleep(Duration::from_millis(10));
    };
    let (sender, receiver) = mpsc::channel();
    let _monitor_connection = monitor
        .connect(
            &output_port,
            "adaptive-ji test capture",
            move |_, bytes, _| {
                let _ = sender.send(bytes.to_vec());
            },
            (),
        )
        .unwrap();

    // Port creation precedes input connection. Probe with a harmless CC until the
    // callback is ready, so the test never relies on an arbitrary startup sleep.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        keyboard.send(&[0xb0, 1, 0]).unwrap();
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(message) if message == [0xbf, 1, 0] => break,
            _ => assert!(Instant::now() < deadline, "Input did not become ready"),
        }
    }

    keyboard.send(&[0x90, 60, 100]).unwrap();
    let c = receive_through(&receiver, [0x91, 60, 100]);
    assert_eq!(c[c.len() - 2], [0xe1, 0, 64]);

    keyboard.send(&[0x90, 64, 100]).unwrap();
    let e = receive_through(&receiver, [0x92, 64, 100]);
    let e_bend = last_bend(&e, 2);
    assert_eq!(e[e.len() - 2], encode_pitch_bend(2, e_bend));
    if matches!(strategy, "five-limit" | "adaptive-fixed") {
        assert_eq!(e_bend, 7631);
    }

    keyboard.send(&[0x90, 67, 100]).unwrap();
    let g = receive_through(&receiver, [0x93, 67, 100]);
    let g_bend = last_bend(&g, 3);
    assert_eq!(g[g.len() - 2], encode_pitch_bend(3, g_bend));
    if matches!(strategy, "five-limit" | "adaptive-fixed") {
        assert_eq!(g_bend, 8272);
    }

    let (previous_e, previous_g) = if strategy != "five-limit" {
        keyboard.send(&[0x90, 70, 100]).unwrap();
        let seventh = receive_through(&receiver, [0x94, 70, 100]);
        let seventh_bend = last_bend(&seventh, 4);
        assert_eq!(
            seventh[seventh.len() - 2],
            encode_pitch_bend(4, seventh_bend)
        );
        if strategy == "adaptive-fixed" {
            let cents = 1200.0 * (7.0_f64 / 4.0).log2() - 1000.0;
            assert_eq!(seventh_bend, cents_to_pitch_bend(cents, 2.0));
        }
        assert_eq!(
            seventh
                .iter()
                .filter(|message| message[0] & 0xf0 == 0x90)
                .count(),
            1
        );
        keyboard.send(&[0x80, 70, 0]).unwrap();
        let released = receive_count(&receiver, 5);
        assert_eq!(released[0], [0x84, 70, 0]);
        assert!(!released.iter().any(|message| message[0] & 0xf0 == 0x90));
        (last_bend(&released, 2), last_bend(&released, 3))
    } else {
        (last_bend(&g, 2), g_bend)
    };

    // E becomes the new reference (adaptive) or root (five-limit).
    keyboard.send(&[0x80, 60, 45]).unwrap();
    let retuned = receive_count(&receiver, 4);
    assert_eq!(retuned[0], [0x81, 60, 45]);
    assert_eq!(retuned[1], encode_pitch_bend(1, 8192));
    assert!(!retuned.iter().any(|message| message[0] & 0xf0 == 0x90));
    let new_e = last_bend(&retuned, 2);
    let new_g = last_bend(&retuned, 3);
    if matches!(strategy, "five-limit" | "adaptive-fixed") {
        assert_eq!(new_e, 8192);
        let ratio: f64 = if strategy == "five-limit" {
            6.0 / 5.0
        } else {
            32.0 / 27.0
        };
        let g_relative_to_e = 1200.0 * ratio.log2() - 300.0;
        assert_eq!(new_g, cents_to_pitch_bend(g_relative_to_e, 2.0));
    } else {
        let cents_per_bend = 200.0 / 8192.0;
        assert!((i32::from(new_e) - i32::from(previous_e)).abs() as f64 * cents_per_bend < 2.0);
        assert!((i32::from(new_g) - i32::from(previous_g)).abs() as f64 * cents_per_bend < 2.0);
        assert_ne!(new_e, 8192);
    }

    assert!(
        Command::new("kill")
            .args(["-INT", &app.0.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let cleanup = receive_through(&receiver, [0xef, 0, 64]);
    assert!(cleanup.contains(&vec![0x82, 64, 0]));
    assert!(cleanup.contains(&vec![0x83, 67, 0]));
    for channel in 0..=15 {
        assert!(cleanup.contains(&vec![0xb0 | channel, 64, 0]));
        assert!(cleanup.contains(&vec![0xb0 | channel, 123, 0]));
    }
    for channel in 1..=15 {
        assert!(cleanup.contains(&encode_pitch_bend(channel, 8192).to_vec()));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = app.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "adaptive-ji did not exit after SIGINT"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

//! Exercise the actual patched backend helper, not a second implementation.
//! ALSA event construction/decoding does not require /dev/snd/seq.
#![cfg(target_os = "linux")]

#[path = "../vendor/midir/src/backend/alsa/timestamp.rs"]
mod timestamp;

use std::time::Duration;

use alsa::seq::{Event, MidiEvent};
use timestamp::event_timestamp_micros;

#[test]
fn direct_events_without_real_time_keep_note_and_panic_bytes() {
    let mut encoder = MidiEvent::new(32).unwrap();
    let decoder = MidiEvent::new(32).unwrap();
    decoder.enable_running_status(false);
    for (index, bytes) in [
        [0x91, 38, 100],
        [0x81, 38, 0],
        [0xb1, 64, 0],
        [0xb1, 120, 0],
        [0xb1, 121, 0],
        [0xb1, 123, 0],
    ]
    .into_iter()
    .enumerate()
    {
        let (_, event) = encoder.encode(&bytes).unwrap();
        let mut event = event.unwrap();
        event.set_direct();
        assert_eq!(event.get_time(), None); // This panicked in midir 0.11.0.
        let now = Duration::from_micros(100 + index as u64);
        assert_eq!(
            event_timestamp_micros(&event, || now),
            now.as_micros() as u64
        );
        let mut decoded = [0; 32];
        let len = decoder.decode(&mut decoded, &mut event).unwrap();
        assert_eq!(&decoded[..len], bytes);
    }
}

#[test]
fn tick_timestamp_uses_arrival_time_not_musical_ticks_as_microseconds() {
    let mut event = Event::new(alsa::seq::EventType::Sensing, &());
    event.schedule_tick(0, false, 960);
    assert_eq!(event.get_time(), None);
    assert_eq!(
        event_timestamp_micros(&event, || Duration::from_micros(123_456)),
        123_456
    );
}

#[test]
fn real_timestamp_is_preserved_without_reading_fallback_clock() {
    let mut event = Event::new(alsa::seq::EventType::Sensing, &());
    event.schedule_real(0, false, Duration::new(42, 123_456_789));
    assert_eq!(
        event_timestamp_micros(&event, || panic!("unexpected clock fallback")),
        42_123_456
    );
}

#[test]
fn missing_timestamp_fallback_is_deterministic_and_saturates() {
    let event = Event::new(alsa::seq::EventType::Sensing, &());
    for micros in [0, 1, 50_000, 1_000_000, u64::MAX] {
        assert_eq!(
            event_timestamp_micros(&event, || Duration::from_micros(micros)),
            micros
        );
    }
    assert_eq!(event_timestamp_micros(&event, || Duration::MAX), u64::MAX);
}

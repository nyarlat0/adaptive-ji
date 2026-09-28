use std::time::Duration;

use alsa::seq::Event;

/// Preserve ALSA real-time timestamps. A tick stamp cannot be converted to
/// microseconds without its sender's tempo/queue context; use monotonic arrival
/// time instead. The fallback is lazy and supplied by the input handler's clock.
pub(super) fn event_timestamp_micros(
    event: &Event<'_>,
    arrival_time: impl FnOnce() -> Duration,
) -> u64 {
    let time = event.get_time().unwrap_or_else(arrival_time);
    time.as_micros().try_into().unwrap_or(u64::MAX)
}

//! Per-voice linear-in-cents output smoothing. No tuning or MIDI transport lives here.

use std::{collections::BTreeMap, time::Duration};

use crate::{pitch_bend::cents_to_pitch_bend, tuning::VoiceId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlideConfig {
    pub duration_ms: u64,
    pub tick_ms: u64,
}

impl Default for GlideConfig {
    fn default() -> Self {
        Self {
            duration_ms: 35,
            tick_ms: 5,
        }
    }
}

impl GlideConfig {
    pub fn validate(self) -> Result<(), String> {
        if self.duration_ms > 2000 {
            return Err("glide-ms must be in 0..=2000".into());
        }
        if !(1..=100).contains(&self.tick_ms) {
            return Err("glide-tick-ms must be in 1..=100".into());
        }
        Ok(())
    }

    fn duration(self) -> Duration {
        Duration::from_millis(self.duration_ms)
    }

    fn tick(self) -> Duration {
        Duration::from_millis(self.tick_ms)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TargetUpdate {
    Immediate(u16),
    Scheduled { from_cents: f64, to_cents: f64 },
    Unchanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BendUpdate {
    pub voice_id: VoiceId,
    pub bend: u16,
}

#[derive(Clone, Copy, Debug)]
struct GlideState {
    start_cents: f64,
    target_cents: f64,
    start_at: Duration,
    next_tick_at: Option<Duration>,
    last_bend: u16,
}

impl GlideState {
    fn current(self, now: Duration, duration: Duration) -> f64 {
        if duration.is_zero() {
            return self.target_cents;
        }
        let progress = (now.saturating_sub(self.start_at).as_secs_f64() / duration.as_secs_f64())
            .clamp(0.0, 1.0);
        self.start_cents + (self.target_cents - self.start_cents) * progress
    }
}

/// Monotonic times are supplied by the owner; tests can advance time without sleeping.
pub struct GlideEngine {
    config: GlideConfig,
    range_semitones: f64,
    voices: BTreeMap<VoiceId, GlideState>,
}

impl GlideEngine {
    pub fn new(config: GlideConfig, range_semitones: f64) -> Self {
        Self {
            config,
            range_semitones,
            voices: BTreeMap::new(),
        }
    }

    pub fn config(&self) -> GlideConfig {
        self.config
    }

    pub fn set_target(&mut self, id: VoiceId, target: f64, now: Duration) -> TargetUpdate {
        let bend = cents_to_pitch_bend(target, self.range_semitones);
        let Some(state) = self.voices.get_mut(&id) else {
            self.voices.insert(
                id,
                GlideState {
                    start_cents: target,
                    target_cents: target,
                    start_at: now,
                    next_tick_at: None,
                    last_bend: bend,
                },
            );
            return TargetUpdate::Immediate(bend);
        };
        if self.config.duration_ms == 0 {
            state.start_cents = target;
            state.target_cents = target;
            state.start_at = now;
            state.next_tick_at = None;
            state.last_bend = bend;
            return TargetUpdate::Immediate(bend);
        }
        if state.target_cents == target {
            return TargetUpdate::Unchanged;
        }
        let from_cents = state.current(now, self.config.duration());
        state.start_cents = from_cents;
        state.target_cents = target;
        state.start_at = now;
        if from_cents == target {
            state.next_tick_at = None;
            if bend != state.last_bend {
                state.last_bend = bend;
                return TargetUpdate::Immediate(bend);
            }
            return TargetUpdate::Unchanged;
        }
        state.next_tick_at = Some(now + self.config.tick().min(self.config.duration()));
        TargetUpdate::Scheduled {
            from_cents,
            to_cents: target,
        }
    }

    pub fn remove(&mut self, id: VoiceId) {
        self.voices.remove(&id);
    }

    pub fn clear(&mut self) {
        self.voices.clear();
    }

    pub fn next_deadline(&self) -> Option<Duration> {
        self.voices
            .values()
            .filter_map(|state| state.next_tick_at)
            .min()
    }

    /// Only due states are sampled. At completion the exact quantized target is used.
    pub fn tick(&mut self, now: Duration) -> Vec<BendUpdate> {
        let mut output = Vec::new();
        for (&voice_id, state) in &mut self.voices {
            if !state.next_tick_at.is_some_and(|deadline| now >= deadline) {
                continue;
            }
            let elapsed = now.saturating_sub(state.start_at);
            let finished = elapsed >= self.config.duration();
            let bend = cents_to_pitch_bend(
                if finished {
                    state.target_cents
                } else {
                    state.current(now, self.config.duration())
                },
                self.range_semitones,
            );
            if bend != state.last_bend {
                state.last_bend = bend;
                output.push(BendUpdate { voice_id, bend });
            }
            state.next_tick_at = if finished {
                None
            } else {
                Some((now + self.config.tick()).min(state.start_at + self.config.duration()))
            };
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn interpolation_clamps_at_start_midpoint_end_and_after_end() {
        let state = GlideState {
            start_cents: 10.0,
            target_cents: -10.0,
            start_at: ms(20),
            next_tick_at: None,
            last_bend: 8192,
        };
        for (time, expected) in [(0, 10.0), (20, 10.0), (35, 0.0), (50, -10.0), (90, -10.0)] {
            assert!((state.current(ms(time), ms(30)) - expected).abs() < 1e-9);
        }
    }

    #[test]
    fn retarget_uses_interpolated_position_not_old_target() {
        let mut engine = GlideEngine::new(
            GlideConfig {
                duration_ms: 40,
                tick_ms: 5,
            },
            2.0,
        );
        assert!(matches!(
            engine.set_target(1, 0.0, ms(0)),
            TargetUpdate::Immediate(_)
        ));
        engine.set_target(1, 20.0, ms(0));
        assert_eq!(
            engine.set_target(1, -20.0, ms(20)),
            TargetUpdate::Scheduled {
                from_cents: 10.0,
                to_cents: -20.0
            }
        );
        assert!((engine.voices[&1].current(ms(20), ms(40)) - 10.0).abs() < 1e-9);
        assert!((engine.voices[&1].current(ms(40), ms(40)) + 5.0).abs() < 1e-9);
    }

    #[test]
    fn retarget_to_current_position_still_emits_unsent_quantized_bend() {
        let mut engine = GlideEngine::new(
            GlideConfig {
                duration_ms: 40,
                tick_ms: 20,
            },
            2.0,
        );
        engine.set_target(1, 0.0, ms(0));
        engine.set_target(1, 20.0, ms(0));
        assert_eq!(
            engine.set_target(1, 5.0, ms(10)),
            TargetUpdate::Immediate(cents_to_pitch_bend(5.0, 2.0))
        );
        assert_eq!(engine.next_deadline(), None);
    }

    #[test]
    fn zero_duration_is_immediate_and_short_duration_finishes_before_tick() {
        let mut instant = GlideEngine::new(
            GlideConfig {
                duration_ms: 0,
                tick_ms: 5,
            },
            2.0,
        );
        instant.set_target(1, 0.0, ms(0));
        assert_eq!(
            instant.set_target(1, 15.0, ms(1)),
            TargetUpdate::Immediate(cents_to_pitch_bend(15.0, 2.0))
        );
        assert_eq!(instant.next_deadline(), None);
        let mut short = GlideEngine::new(
            GlideConfig {
                duration_ms: 2,
                tick_ms: 5,
            },
            2.0,
        );
        short.set_target(1, 0.0, ms(0));
        short.set_target(1, 15.0, ms(0));
        assert_eq!(short.next_deadline(), Some(ms(2)));
        assert_eq!(short.tick(ms(2))[0].bend, cents_to_pitch_bend(15.0, 2.0));
    }

    #[test]
    fn suppresses_duplicate_bends_but_emits_exact_final_target() {
        let mut engine = GlideEngine::new(
            GlideConfig {
                duration_ms: 20,
                tick_ms: 1,
            },
            2.0,
        );
        engine.set_target(1, 0.0, ms(0));
        engine.set_target(1, 0.02, ms(0));
        assert!(engine.tick(ms(1)).is_empty());
        let mut updates = Vec::new();
        for time in 2..=20 {
            updates.extend(engine.tick(ms(time)));
        }
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].bend, cents_to_pitch_bend(0.02, 2.0));
        assert_eq!(engine.next_deadline(), None);
    }

    #[test]
    fn voices_move_independently_and_removed_ids_never_tick() {
        let mut engine = GlideEngine::new(
            GlideConfig {
                duration_ms: 20,
                tick_ms: 5,
            },
            2.0,
        );
        engine.set_target(1, 0.0, ms(0));
        engine.set_target(2, 0.0, ms(0));
        engine.set_target(1, 20.0, ms(0));
        engine.set_target(2, -20.0, ms(0));
        assert_eq!(engine.tick(ms(10)).len(), 2);
        engine.remove(1);
        assert_eq!(
            engine.tick(ms(20)),
            vec![BendUpdate {
                voice_id: 2,
                bend: cents_to_pitch_bend(-20.0, 2.0)
            }]
        );
        engine.clear();
        assert!(engine.tick(ms(100)).is_empty());
    }

    #[test]
    fn config_bounds() {
        assert!(GlideConfig::default().validate().is_ok());
        assert!(
            GlideConfig {
                duration_ms: 2001,
                tick_ms: 5
            }
            .validate()
            .is_err()
        );
        assert!(
            GlideConfig {
                duration_ms: 35,
                tick_ms: 0
            }
            .validate()
            .is_err()
        );
        assert!(
            GlideConfig {
                duration_ms: 35,
                tick_ms: 101
            }
            .validate()
            .is_err()
        );
    }
}

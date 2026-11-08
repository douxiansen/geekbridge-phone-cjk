// SPDX-License-Identifier: GPL-3.0-or-later

//! Easing curves and tweens. All time is passed in, never read from a clock,
//! so animations are testable and deterministic.

use std::time::{Duration, Instant};

/// Slide-in duration.
pub const ENTER: Duration = Duration::from_millis(280);
/// Slide-out duration.
pub const LEAVE: Duration = Duration::from_millis(200);
/// Reply field expand/collapse duration.
pub const EXPAND: Duration = Duration::from_millis(180);
/// Button hover/press transition.
pub const PRESS: Duration = Duration::from_millis(110);
/// Loop length of the ripple dots.
pub const RIPPLE_PERIOD: Duration = Duration::from_millis(1200);

/// Clamp to the unit interval.
pub fn clamp01(t: f32) -> f32 {
    if t.is_nan() {
        0.0
    } else {
        t.clamp(0.0, 1.0)
    }
}

/// Decelerating cubic: fast start, soft landing.
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = clamp01(t);
    1.0 - (1.0 - t).powi(3)
}

/// Accelerating cubic, for things leaving.
pub fn ease_in_cubic(t: f32) -> f32 {
    let t = clamp01(t);
    t * t * t
}

/// Symmetric ease.
pub fn ease_in_out_cubic(t: f32) -> f32 {
    let t = clamp01(t);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Which curve a [`Tween`] follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ease {
    /// [`ease_out_cubic`].
    Out,
    /// [`ease_in_cubic`].
    In,
    /// [`ease_in_out_cubic`].
    InOut,
}

impl Ease {
    fn apply(self, t: f32) -> f32 {
        match self {
            Ease::Out => ease_out_cubic(t),
            Ease::In => ease_in_cubic(t),
            Ease::InOut => ease_in_out_cubic(t),
        }
    }
}

/// A value moving between two points in time.
#[derive(Debug, Clone, Copy)]
pub struct Tween {
    from: f32,
    to: f32,
    start: Instant,
    dur: Duration,
    ease: Ease,
}

impl Tween {
    /// A tween that is already at rest on `v`.
    pub fn at(v: f32, now: Instant) -> Self {
        Self {
            from: v,
            to: v,
            start: now,
            dur: Duration::ZERO,
            ease: Ease::Out,
        }
    }

    /// Value at `now`.
    pub fn value(&self, now: Instant) -> f32 {
        if self.dur.is_zero() {
            return self.to;
        }
        let elapsed = now.saturating_duration_since(self.start);
        let t = elapsed.as_secs_f32() / self.dur.as_secs_f32();
        self.from + (self.to - self.from) * self.ease.apply(t)
    }

    /// Where the tween is heading.
    pub fn target(&self) -> f32 {
        self.to
    }

    /// Whether it still moves at `now`.
    pub fn is_running(&self, now: Instant) -> bool {
        !self.dur.is_zero() && now.saturating_duration_since(self.start) < self.dur
    }

    /// Retarget from the current value (no jump) over `dur`.
    pub fn retarget(&mut self, to: f32, now: Instant, dur: Duration, ease: Ease) {
        if (to - self.to).abs() < f32::EPSILON {
            return;
        }
        self.from = self.value(now);
        self.to = to;
        self.start = now;
        self.dur = dur;
        self.ease = ease;
    }
}

/// Position in a loop of `period`, in `0.0..1.0`.
pub fn loop_phase(elapsed: Duration, period: Duration) -> f32 {
    if period.is_zero() {
        return 0.0;
    }
    let p = elapsed.as_secs_f32() % period.as_secs_f32();
    p / period.as_secs_f32()
}

/// Brightness `0.0..=1.0` of dot `i` of `n` in the ripple at loop phase `phase`:
/// a soft pulse travelling left to right, wrapping around.
pub fn ripple_level(i: usize, n: usize, phase: f32) -> f32 {
    if n == 0 {
        return 0.0;
    }
    // Raised-cosine bump, `w` dots wide. The centre sweeps `w` past both ends,
    // so every dot is dark at phase 0 and 1 and the loop is seamless.
    let w = 1.6;
    let span = (n - 1) as f32 + 2.0 * w;
    let centre = phase * span - w;
    let d = (i as f32 - centre).abs();
    if d >= w {
        0.0
    } else {
        0.5 * (1.0 + (std::f32::consts::PI * d / w).cos())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_endpoints_and_monotonic() {
        for f in [ease_out_cubic, ease_in_cubic, ease_in_out_cubic] {
            assert_eq!(f(0.0), 0.0);
            assert_eq!(f(1.0), 1.0);
            assert_eq!(f(-3.0), 0.0);
            assert_eq!(f(9.0), 1.0);
            let mut last = 0.0;
            for i in 0..=100 {
                let v = f(i as f32 / 100.0);
                assert!(v >= last - 1e-6);
                last = v;
            }
        }
        assert!(ease_out_cubic(0.5) > 0.5);
        assert!(ease_in_cubic(0.5) < 0.5);
        assert_eq!(ease_out_cubic(f32::NAN), 0.0);
    }

    #[test]
    fn tween_runs_and_retargets_without_jump() {
        let t0 = Instant::now();
        let mut tw = Tween::at(0.0, t0);
        assert!(!tw.is_running(t0));
        tw.retarget(10.0, t0, Duration::from_millis(100), Ease::Out);
        assert!(tw.is_running(t0 + Duration::from_millis(50)));
        let mid = tw.value(t0 + Duration::from_millis(50));
        assert!(mid > 5.0 && mid < 10.0);
        assert_eq!(tw.value(t0 + Duration::from_millis(100)), 10.0);
        assert!(!tw.is_running(t0 + Duration::from_millis(100)));

        let now = t0 + Duration::from_millis(50);
        tw.retarget(0.0, now, Duration::from_millis(100), Ease::Out);
        assert!((tw.value(now) - mid).abs() < 1e-4);
    }

    #[test]
    fn ripple_loops_and_pulses() {
        let sum = |p: f32| (0..7).map(|i| ripple_level(i, 7, p)).sum::<f32>();
        assert!(sum(0.5) > 0.9);
        // Seamless: phase 0 and 1 look the same.
        for i in 0..7 {
            assert!(ripple_level(i, 7, 0.0) < 0.01);
            assert!(ripple_level(i, 7, 1.0) < 0.01);
        }
        assert_eq!(ripple_level(0, 0, 0.3), 0.0);
        assert!((loop_phase(Duration::from_millis(1800), RIPPLE_PERIOD) - 0.5).abs() < 1e-4);
    }
}

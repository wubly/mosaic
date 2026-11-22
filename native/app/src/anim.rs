use std::time::{Duration, Instant};

const SUBSTEP: f32 = 1.0 / 240.0;

/// `MOSAIC_REDUCED_MOTION=1` snaps every spring and tween to its target.
pub fn reduced_motion() -> bool {
    std::env::var_os("MOSAIC_REDUCED_MOTION").is_some_and(|v| v == "1")
}

/// Solves a CSS cubic-bezier at progress `x`.
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let coord = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if coord(x1, x2, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    coord(y1, y2, (lo + hi) / 2.0)
}

/// A damped spring with the same parameters as framer-motion (`mass = 1`).
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl Spring {
    pub fn new(value: f32, stiffness: f32, damping: f32) -> Self {
        Spring {
            value,
            velocity: 0.0,
            target: value,
            stiffness,
            damping,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
        if reduced_motion() {
            self.value = target;
            self.velocity = 0.0;
        }
    }

    pub fn is_settled(&self) -> bool {
        self.velocity.abs() < 0.01 && (self.value - self.target).abs() < 0.001
    }

    /// Semi-implicit Euler in fixed 1/240 s substeps.
    pub fn step(&mut self, dt: Duration) {
        if self.is_settled() {
            self.value = self.target;
            self.velocity = 0.0;
            return;
        }
        let mut left = dt.as_secs_f32().min(0.1);
        while left > 0.0 {
            let h = left.min(SUBSTEP);
            let force = -self.stiffness * (self.value - self.target) - self.damping * self.velocity;
            self.velocity += force * h;
            self.value += self.velocity * h;
            left -= h;
        }
        if self.is_settled() {
            self.value = self.target;
            self.velocity = 0.0;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ease {
    Linear,
    /// `cubic-bezier(.3, 0, 0, 1)`, used for the wavy amplitude.
    Out,
    SineIn,
    SineOut,
}

impl Ease {
    pub fn apply(self, t: f32) -> f32 {
        match self {
            Ease::Linear => t,
            Ease::Out => cubic_bezier(0.3, 0.0, 0.0, 1.0, t),
            Ease::SineIn => (t * std::f32::consts::FRAC_PI_2).sin(),
            Ease::SineOut => 1.0 - (t * std::f32::consts::FRAC_PI_2).cos(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Tween {
    pub from: f32,
    pub to: f32,
    start: Instant,
    dur: Duration,
    ease: Ease,
}

impl Tween {
    pub fn new(from: f32, to: f32, start: Instant, dur: Duration, ease: Ease) -> Self {
        let dur = if reduced_motion() {
            Duration::ZERO
        } else {
            dur
        };
        Tween {
            from,
            to,
            start,
            dur,
            ease,
        }
    }

    pub fn at(&self, now: Instant) -> f32 {
        if self.dur.is_zero() {
            return self.to;
        }
        let t = (now.saturating_duration_since(self.start).as_secs_f32() / self.dur.as_secs_f32())
            .clamp(0.0, 1.0);
        self.from + (self.to - self.from) * self.ease.apply(t)
    }

    pub fn is_done(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) >= self.dur
    }
}

#[derive(Clone, Copy, Debug)]
enum Motion {
    Spring(Spring),
    Tween(Tween),
}

/// Animated values keyed by a stable id; `is_active` drives the frame subscription.
#[derive(Default)]
pub struct AnimationClock {
    values: std::collections::HashMap<&'static str, (f32, Motion)>,
    last: Option<Instant>,
}

impl AnimationClock {
    pub fn spring(&mut self, key: &'static str, target: f32, stiffness: f32, damping: f32) {
        let value = self.values.get(key).map_or(target, |(v, _)| *v);
        let mut spring = match self.values.get(key) {
            Some((_, Motion::Spring(s))) => *s,
            _ => Spring::new(value, stiffness, damping),
        };
        spring.set_target(target);
        self.values
            .insert(key, (spring.value, Motion::Spring(spring)));
    }

    pub fn tween(
        &mut self,
        key: &'static str,
        target: f32,
        now: Instant,
        dur: Duration,
        ease: Ease,
    ) {
        let from = self.get(key).unwrap_or(target);
        if self
            .values
            .get(key)
            .is_some_and(|(_, m)| matches!(m, Motion::Tween(t) if t.to == target))
        {
            return;
        }
        let tween = Tween::new(from, target, now, dur, ease);
        self.values.insert(key, (from, Motion::Tween(tween)));
    }

    pub fn get(&self, key: &str) -> Option<f32> {
        self.values.get(key).map(|(v, _)| *v)
    }

    pub fn value(&self, key: &str, default: f32) -> f32 {
        self.get(key).unwrap_or(default)
    }

    pub fn tick(&mut self, now: Instant) {
        let dt = self
            .last
            .map_or(Duration::ZERO, |l| now.saturating_duration_since(l));
        self.last = Some(now);
        for (value, motion) in self.values.values_mut() {
            match motion {
                Motion::Spring(s) => {
                    s.step(dt);
                    *value = s.value;
                }
                Motion::Tween(t) => *value = t.at(now),
            }
        }
    }

    pub fn is_active(&self, now: Instant) -> bool {
        self.values.values().any(|(_, m)| match m {
            Motion::Spring(s) => !s.is_settled(),
            Motion::Tween(t) => !t.is_done(now),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_converges_without_overshooting_a_critically_damped_target() {
        let mut s = Spring::new(0.0, 260.0, 30.0);
        s.set_target(1.0);
        let mut peak = 0.0f32;
        for _ in 0..600 {
            s.step(Duration::from_millis(16));
            peak = peak.max(s.value);
        }
        assert!(s.is_settled());
        assert_eq!(s.value, 1.0);
        assert!(peak < 1.05, "260/30 is close to critical: peak {peak}");
    }

    #[test]
    fn underdamped_spring_overshoots() {
        let mut s = Spring::new(0.0, 140.0, 20.0);
        s.set_target(1.0);
        let mut peak = 0.0f32;
        for _ in 0..300 {
            s.step(Duration::from_millis(16));
            peak = peak.max(s.value);
        }
        assert!(peak > 1.0);
    }

    #[test]
    fn settled_spring_does_not_move() {
        let mut s = Spring::new(5.0, 100.0, 10.0);
        s.step(Duration::from_secs(1));
        assert_eq!(s.value, 5.0);
        assert!(s.is_settled());
    }

    #[test]
    fn tween_follows_its_ease() {
        let now = Instant::now();
        let t = Tween::new(0.0, 10.0, now, Duration::from_millis(100), Ease::Linear);
        assert_eq!(t.at(now), 0.0);
        assert!((t.at(now + Duration::from_millis(50)) - 5.0).abs() < 1e-4);
        assert_eq!(t.at(now + Duration::from_millis(200)), 10.0);
        assert!(t.is_done(now + Duration::from_millis(100)));
        let s = Tween::new(0.0, 1.0, now, Duration::from_millis(100), Ease::SineIn);
        assert!(
            (s.at(now + Duration::from_millis(50)) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3
        );
        let o = Tween::new(0.0, 1.0, now, Duration::from_millis(100), Ease::SineOut);
        assert!((o.at(now + Duration::from_millis(50)) - 0.2929).abs() < 1e-3);
    }

    #[test]
    fn bezier_hits_endpoints_and_is_monotonic() {
        assert!(cubic_bezier(0.25, 0.1, 0.25, 1.0, 0.0).abs() < 1e-3);
        assert!((cubic_bezier(0.25, 0.1, 0.25, 1.0, 1.0) - 1.0).abs() < 1e-3);
        let mut last = 0.0;
        for i in 0..=20 {
            let v = cubic_bezier(0.3, 0.0, 0.0, 1.0, i as f32 / 20.0);
            assert!(v >= last - 1e-4);
            last = v;
        }
    }

    #[test]
    fn clock_reports_activity_until_everything_settles() {
        let mut clock = AnimationClock::default();
        let now = Instant::now();
        clock.spring("pill", 0.0, 500.0, 36.0);
        assert!(!clock.is_active(now));
        clock.spring("pill", 1.0, 500.0, 36.0);
        assert!(clock.is_active(now));
        let mut t = now;
        for _ in 0..200 {
            t += Duration::from_millis(16);
            clock.tick(t);
        }
        assert!(!clock.is_active(t));
        assert_eq!(clock.value("pill", 0.0), 1.0);
    }
}

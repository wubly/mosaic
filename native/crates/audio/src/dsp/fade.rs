use crate::{AfterFade, Curve};

pub fn ease(curve: Curve, t: f32) -> f32 {
    match curve {
        Curve::Linear => t,
        Curve::In => (t * std::f32::consts::FRAC_PI_2).sin(),
        Curve::Out => 1.0 - (t * std::f32::consts::FRAC_PI_2).cos(),
    }
}

/// A running volume ramp, advanced one output frame at a time.
#[derive(Clone, Copy, Debug)]
pub struct FadeRun {
    start: f32,
    to: f32,
    len: u32,
    done: u32,
    curve: Curve,
    pub then: AfterFade,
}

impl FadeRun {
    pub fn new(start: f32, to: f32, len: u32, curve: Curve, then: AfterFade) -> FadeRun {
        FadeRun {
            start,
            to,
            len: len.max(1),
            done: 0,
            curve,
            then,
        }
    }

    /// Gain for the next frame, and whether the ramp has just finished.
    pub fn step(&mut self) -> (f32, bool) {
        self.done += 1;
        if self.done >= self.len {
            return (self.to, true);
        }
        let t = self.done as f32 / self.len as f32;
        (
            self.start + (self.to - self.start) * ease(self.curve, t),
            false,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_are_equal_power_pair() {
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            let fade_in = ease(Curve::In, t);
            let fade_out = 1.0 - ease(Curve::Out, t);
            assert!((fade_in * fade_in + fade_out * fade_out - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn ramp_reaches_target() {
        let mut f = FadeRun::new(0.0, 1.0, 4, Curve::Linear, AfterFade::Nothing);
        let steps: Vec<_> = (0..4).map(|_| f.step()).collect();
        assert_eq!(steps[3], (1.0, true));
        assert!((steps[0].0 - 0.25).abs() < 1e-6);
        assert!(!steps[2].1);
    }
}

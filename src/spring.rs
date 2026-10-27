//! Damped spring, the same model Framer Motion uses for `type: "spring"`.
//!
//! Integrated with fixed sub-steps so the result doesn't depend on frame rate and
//! a long frame (window dragged, GPU stall) can't blow the simulation up.

#[derive(Clone, Copy, Debug)]
pub struct SpringConfig {
    pub stiffness: f32,
    pub damping: f32,
    pub mass: f32,
}

impl SpringConfig {
    /// Snappy with a little overshoot; close to what the Dynamic Island does.
    pub const ISLAND: Self = Self {
        stiffness: 420.0,
        damping: 28.0,
        mass: 1.0,
    };
}

#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    config: SpringConfig,
}

const SUBSTEP: f32 = 1.0 / 240.0;
const MAX_FRAME: f32 = 0.1;
const REST_DELTA: f32 = 0.05;
const REST_SPEED: f32 = 0.5;

impl Spring {
    pub fn new(value: f32, config: SpringConfig) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
            config,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn is_settled(&self) -> bool {
        (self.target - self.value).abs() < REST_DELTA && self.velocity.abs() < REST_SPEED
    }

    /// Advance by `dt` seconds. Returns true while still moving.
    pub fn step(&mut self, dt: f32) -> bool {
        if self.is_settled() {
            self.value = self.target;
            self.velocity = 0.0;
            return false;
        }
        let mut remaining = dt.clamp(0.0, MAX_FRAME);
        while remaining > 0.0 {
            let h = remaining.min(SUBSTEP);
            let displacement = self.value - self.target;
            let accel = (-self.config.stiffness * displacement
                - self.config.damping * self.velocity)
                / self.config.mass;
            // Semi-implicit Euler: stable for stiff springs at this step size.
            self.velocity += accel * h;
            self.value += self.velocity * h;
            remaining -= h;
        }
        if self.is_settled() {
            self.value = self.target;
            self.velocity = 0.0;
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_to_rest(spring: &mut Spring, frame: f32) -> f32 {
        let mut t = 0.0;
        while spring.step(frame) {
            t += frame;
            assert!(t < 10.0, "spring never settled");
        }
        t
    }

    #[test]
    fn settles_exactly_on_target() {
        let mut s = Spring::new(0.0, SpringConfig::ISLAND);
        s.set_target(100.0);
        run_to_rest(&mut s, 1.0 / 60.0);
        assert_eq!(s.value, 100.0);
        assert_eq!(s.velocity, 0.0);
    }

    #[test]
    fn underdamped_island_spring_overshoots() {
        let mut s = Spring::new(0.0, SpringConfig::ISLAND);
        s.set_target(100.0);
        let mut peak: f32 = 0.0;
        while s.step(1.0 / 120.0) {
            peak = peak.max(s.value);
        }
        assert!(peak > 100.0, "expected overshoot, peak was {peak}");
        assert!(peak < 120.0, "overshoot should be modest, peak was {peak}");
    }

    #[test]
    fn frame_rate_independent() {
        let settle = |frame: f32| {
            let mut s = Spring::new(0.0, SpringConfig::ISLAND);
            s.set_target(100.0);
            let mut v = 0.0;
            for _ in 0..((0.15 / frame) as usize) {
                s.step(frame);
                v = s.value;
            }
            v
        };
        let at_60 = settle(1.0 / 60.0);
        let at_144 = settle(1.0 / 144.0);
        assert!((at_60 - at_144).abs() < 2.0, "{at_60} vs {at_144}");
    }

    #[test]
    fn huge_frame_does_not_explode() {
        let mut s = Spring::new(0.0, SpringConfig::ISLAND);
        s.set_target(100.0);
        s.step(5.0);
        assert!(s.value.is_finite() && s.value.abs() < 1000.0);
    }

    #[test]
    fn retargeting_mid_flight_keeps_velocity() {
        let mut s = Spring::new(0.0, SpringConfig::ISLAND);
        s.set_target(100.0);
        for _ in 0..6 {
            s.step(1.0 / 60.0);
        }
        let v = s.velocity;
        assert!(v > 0.0);
        s.set_target(0.0);
        assert_eq!(s.velocity, v, "interrupting must not reset velocity");
        run_to_rest(&mut s, 1.0 / 60.0);
        assert_eq!(s.value, 0.0);
    }
}

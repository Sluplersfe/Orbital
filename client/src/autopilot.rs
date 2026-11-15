//! Scripted ascent used by tests (and later as an unlockable instrument).
//! Gravity turn, cut at target apoapsis, coast, circularise.
use crate::sim::{Flight, Status};
use crate::{MU, R_EARTH};

pub struct Ascent {
    pub target_alt: f64,
    pub turn_start: f64,
    pub turn_end: f64,
    pub pitch_end: f64,
    circ_started: bool,
    pub kick: f64,
}

impl Ascent {
    pub fn new(target_alt: f64) -> Self {
        Self { target_alt, turn_start: 250.0, turn_end: 45_000.0, pitch_end: 88.0, circ_started: false, kick: 2.0 }
    }

    /// Drive one control tick (call every ~0.1-1 s of sim time).
    pub fn control(&mut self, f: &mut Flight) {
        let alt = f.alt();
        if f.status == Status::OnPad {
            f.set_throttle(1.0);
        }
        // Gravity turn: small kick, then follow the surface-relative velocity;
        // above ~35 km blend toward the horizon.
        let up = f.pos.normalize();
        let east = bevy::math::DVec3::Z.cross(up).normalize();
        let vr = f.rel_vel();
        let speed = vr.length();
        let pro = vr.dot(east).atan2(vr.dot(up)).to_degrees();
        let blend = if alt < 35_000.0 { 0.0 } else { ((f.apo_alt() - 0.6 * self.target_alt) / (0.4 * self.target_alt)).clamp(0.0, 1.0) };
        let base = if alt < self.turn_start { 0.0 } else if speed < 110.0 { self.kick } else { pro.clamp(0.0, 90.0) };
        f.set_pitch(base.max(self.pitch_end * blend));
        if f.stage_empty() && f.stage + 1 < f.vessel.stages.len() && f.throttle > 0.0 {
            f.stage();
        }
        // Throttle limiter: stay under 85% of the weakest part's max-Q.
        let limit = f.vessel.weakest().map_or(f64::MAX, |w| w.1) * 0.85;
        let q = f.q();
        let cap = if q > limit { 0.0 } else if q > limit * 0.6 { (limit - q) / (limit * 0.4) } else { 1.0 };
        let apo = f.apo_alt();
        let target_r = R_EARTH + self.target_alt;
        if !self.circ_started {
            if apo >= self.target_alt {
                f.set_throttle(0.0);
                if alt > 70_000.0 {
                    // coast: look prograde-horizontal
                    f.set_pitch(90.0);
                    let r = f.pos.length();
                    let v = f.vel.length();
                    let ta = -(MU / r) / (2.0 * (v * v / 2.0 - MU / r)) * (1.0 + f.elements().e);
                    let _ = ta;
                    // start circularisation near apoapsis (radial speed ~0 and climbing stopped)
                    if f.vel.dot(f.pos) <= 0.0 || r >= target_r * 0.999 {
                        self.circ_started = true;
                    }
                }
            } else {
                f.set_throttle(cap.max(0.05));
            }
        } else {
            // Hold altitude: tip the nose up as the vessel starts to sink.
            let vr = f.vel.dot(f.pos.normalize());
            f.set_pitch(90.0 - (-vr * 0.2).clamp(-20.0, 40.0));
            let r = f.pos.length();
            let v_circ = (MU / r).sqrt();
            if f.vel.length() < v_circ && f.peri_alt() < self.target_alt * 0.9 {
                f.set_throttle(1.0);
                if f.stage_empty() && f.stage + 1 < f.vessel.stages.len() {
                    f.stage();
                }
            } else {
                f.set_throttle(0.0);
            }
        }
    }

    /// Fly until over, orbit reached, or a time limit.
    pub fn run(&mut self, f: &mut Flight, limit_s: f64) {
        while f.t < limit_s && !f.status.is_over() && !f.reached_orbit {
            self.control(f);
            let dt = if f.throttle == 0.0 && f.alt() > 75_000.0 { 0.5 } else { 0.05 };
            f.advance(0.1, dt);
        }
    }
}

/// Lift-off from an airless body to a circular parking orbit: straight up, a pitch-over that
/// follows altitude, a coast to apoapsis, then a circularising burn.
pub struct Takeoff {
    pub target_alt: f64,
    circ: bool,
}

impl Takeoff {
    pub fn new(target_alt: f64) -> Self {
        Self { target_alt, circ: false }
    }

    pub fn control(&mut self, f: &mut Flight) {
        let alt = f.alt();
        if f.status == Status::OnPad {
            f.set_throttle(1.0);
        }
        if f.stage_empty() && f.stage + 1 < f.vessel.stages.len() && f.throttle > 0.0 {
            f.stage();
        }
        if !self.circ {
            // Pitch over smoothly with altitude; never fully horizontal while still low.
            let frac = (alt / (0.5 * self.target_alt)).clamp(0.0, 1.0);
            let pitch = if alt < 150.0 { 0.0 } else { (90.0 * frac.powf(0.6)).clamp(3.0, 88.0) };
            f.set_pitch(pitch);
            if f.apo_alt() >= self.target_alt {
                f.set_throttle(0.0);
                if alt > 0.5 * self.target_alt && f.vel.dot(f.pos) <= 0.0 {
                    self.circ = true;
                }
                if alt > 0.9 * self.target_alt {
                    f.set_pitch(90.0);
                }
            } else {
                f.set_throttle(1.0);
            }
        } else {
            let vr = f.vel.dot(f.pos.normalize());
            f.set_pitch(90.0 - (-vr * 0.2).clamp(-20.0, 40.0));
            let v_circ = (f.body.mu / f.pos.length()).sqrt();
            if f.vel.length() < v_circ && f.peri_alt() < self.target_alt * 0.9 {
                f.set_throttle(1.0);
                if f.stage_empty() && f.stage + 1 < f.vessel.stages.len() {
                    f.stage();
                }
            } else {
                f.set_throttle(0.0);
            }
        }
    }

    pub fn run(&mut self, f: &mut Flight, limit_s: f64) {
        while f.t < limit_s && !f.status.is_over() && !f.reached_orbit {
            self.control(f);
            let dt = if f.throttle == 0.0 && f.alt() > 0.5 * self.target_alt { 0.5 } else { 0.05 };
            f.advance(0.1, dt);
        }
    }
}

/// Scripted powered descent from a circular orbit to the surface of an airless body (used by
/// tests and as a later instrument): a retrograde deorbit burn to a low periapsis, a coast, a
/// braking burn timed so that the ballistic path would reach the ground just as the burn would
/// end, then a gentle vertical touchdown.
pub struct Descent {
    pub target_peri: f64,
    phase: u8,
}

impl Default for Descent {
    fn default() -> Self {
        Self::new()
    }
}

impl Descent {
    pub fn new() -> Self {
        Self { target_peri: 12_000.0, phase: 0 }
    }

    /// Seconds until the lowest point of the current path.
    fn time_to_periapsis(f: &Flight) -> Option<f64> {
        let mu = f.body.mu;
        let period = f.elements().period(mu);
        if !period.is_finite() {
            return None;
        }
        let r = |t: f64| crate::orbit::propagate(mu, f.pos, f.vel, t).0.length();
        let steps = 240;
        let dt = period / steps as f64;
        let (i, _) = (0..=steps).map(|i| (i, r(i as f64 * dt))).min_by(|a, b| a.1.total_cmp(&b.1))?;
        // Refine around the coarse minimum.
        let (lo, hi) = ((i as f64 - 1.0).max(0.0) * dt, (i as f64 + 1.0) * dt);
        (0..=80).map(|j| lo + (hi - lo) * j as f64 / 80.0).map(|t| (t, r(t))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|x| x.0)
    }

    pub fn control(&mut self, f: &mut Flight) {
        use bevy::math::DVec3;
        let body = f.body;
        let up = f.pos.normalize();
        let east = DVec3::Z.cross(up).normalize();
        let vr = f.rel_vel();
        let aim = |f: &mut Flight, dir: DVec3| {
            let pitch = dir.dot(east).atan2(dir.dot(up)).to_degrees();
            f.set_pitch(pitch);
        };
        if f.stage_empty() && f.stage + 1 < f.vessel.stages.len() && f.throttle > 0.0 {
            f.stage();
        }
        let thrust = f.vessel.stage_engines(f.stage).0 * 1000.0;
        let a_max = thrust / f.mass();
        let g = body.surface_gravity();
        let (v, vz) = (vr.length(), vr.dot(up));
        let horiz = (vr - up * vz).length();
        match self.phase {
            0 => {
                // Deorbit: burn retrograde until the periapsis is low enough.
                aim(f, -f.vel.normalize());
                let excess = f.peri_alt() - self.target_peri;
                if excess <= 0.0 {
                    f.set_throttle(0.0);
                    self.phase = 1;
                } else if (f.pitch - f.pitch_cmd).abs() < 3.0 {
                    f.set_throttle((excess / 8_000.0).clamp(0.02, 1.0));
                } else {
                    f.set_throttle(0.0);
                }
            }
            1 => {
                // Coast; start braking so that the burn is centred on periapsis.
                f.set_throttle(0.0);
                aim(f, -vr.normalize());
                let decel = (a_max * 0.9 - g * 0.6).max(0.2);
                let t_burn = v / decel;
                if Self::time_to_periapsis(f).is_some_and(|t| t <= t_burn * 0.35 + 10.0) {
                    self.phase = 2;
                }
            }
            2 => {
                // Braking: full thrust against the sideways motion, tilted up to cancel gravity and
                // steer the sink rate toward a slow profile, until the sideways speed is gone.
                let horiz_dir = if horiz > 1.0 { (vr - up * vz) / horiz } else { east };
                let vz_target = -(f.alt() * 0.02).clamp(0.0, 60.0);
                let a_up = (g + 0.8 * (vz_target - vz)).clamp(-a_max * 0.5, a_max * 0.95);
                let dir = -horiz_dir * (a_max * a_max - a_up * a_up).max(1.0).sqrt() + up * a_up;
                aim(f, dir.normalize());
                f.set_throttle(1.0);
                if horiz < 60.0 {
                    self.phase = 3;
                }
            }
            _ => {
                // Terminal: kill sideways speed, then follow a sink-rate profile to the ground.
                let alt = f.alt().max(0.0);
                if horiz > 3.0 {
                    aim(f, -vr.normalize());
                } else {
                    f.set_pitch(0.0);
                }
                let spare = (a_max * 0.9 - g).max(0.3);
                let target = -(1.0 + (2.0 * 0.5 * spare * alt).sqrt()).min(150.0);
                let want = g + 1.5 * (target - vz) + if horiz > 3.0 { horiz * 0.6 } else { 0.0 };
                f.set_throttle((want / a_max).clamp(0.0, 1.0));
            }
        }
    }

    pub fn run(&mut self, f: &mut Flight, limit_s: f64) {
        while f.t < limit_s && !f.status.is_over() {
            self.control(f);
            let dt = if f.alt() < 3_000.0 || self.phase >= 2 { 0.02 } else if self.phase == 0 { 0.1 } else { 0.5 };
            f.advance(0.1, dt);
        }
    }
}

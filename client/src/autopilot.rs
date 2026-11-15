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

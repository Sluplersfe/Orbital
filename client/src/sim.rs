//! 64-bit flight simulation: thrust, gravity, atmosphere, drag, staging, max-Q, landing.
use crate::orbit::Elements;
use crate::parts::Kind;
use crate::vessel::Vessel;
use crate::{ATMO_TOP, G0, MU, R_EARTH, RHO0, SCALE_H, SPACE_ALT};
use bevy::math::DVec3;

pub const OMEGA: f64 = 7.2921159e-5;
const REF_AREA: f64 = 10.0;
const CHUTE_CDA: f64 = 1.5 * 220.0;
pub const SAFE_LANDING: f64 = 9.0;
const SLEW_DEG_S: f64 = 15.0;
pub const ORBIT_PERIAPSIS: f64 = 80_000.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    OnPad,
    Flying,
    Landed { speed: f64 },
    Crashed { speed: f64 },
    BrokeUp { q: f64, limit: f64, part: &'static str },
    Ended,
}

impl Status {
    pub fn is_over(&self) -> bool {
        !matches!(self, Status::OnPad | Status::Flying)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Sample {
    pub t: f64,
    pub alt: f64,
    pub speed: f64,
    pub q: f64,
    pub mass: f64,
    pub apo: f64,
    pub peri: f64,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub t: f64,
    pub alt: f64,
    pub msg: String,
}

#[derive(Debug, Clone)]
pub struct Flight {
    pub vessel: Vessel,
    pub status: Status,
    pub t: f64,
    pub pos: DVec3,
    pub vel: DVec3,
    pub stage: usize,
    pub prop: f64,
    pub throttle: f64,
    /// Commanded / actual pitch from vertical toward east (deg).
    pub pitch_cmd: f64,
    pub pitch: f64,
    pub chute_deployed: bool,
    pub chute_torn: bool,
    // Report bookkeeping
    pub max_alt: f64,
    pub max_speed: f64,
    pub max_q: f64,
    pub max_q_t: f64,
    pub max_q_alt: f64,
    pub dv_used: f64,
    pub liftoff_twr: f64,
    pub reached_space: bool,
    pub reached_orbit: bool,
    pub impact_speed: Option<f64>,
    pub events: Vec<Event>,
    pub samples: Vec<Sample>,
    next_sample: f64,
}

impl Flight {
    pub fn new(vessel: Vessel) -> Self {
        let pos = DVec3::new(R_EARTH, 0.0, 0.0);
        let prop = vessel.stage_prop(0);
        let mut f = Self {
            vessel, status: Status::OnPad, t: 0.0, pos, vel: DVec3::ZERO, stage: 0, prop,
            throttle: 0.0, pitch_cmd: 0.0, pitch: 0.0, chute_deployed: false, chute_torn: false,
            max_alt: 0.0, max_speed: 0.0, max_q: 0.0, max_q_t: 0.0, max_q_alt: 0.0, dv_used: 0.0,
            liftoff_twr: 0.0, reached_space: false, reached_orbit: false, impact_speed: None,
            events: vec![], samples: vec![], next_sample: 0.0,
        };
        f.vel = f.ground_vel();
        f
    }

    fn ground_vel(&self) -> DVec3 {
        DVec3::Z.cross(self.pos) * OMEGA
    }

    pub fn alt(&self) -> f64 {
        self.pos.length() - R_EARTH
    }

    pub fn rel_vel(&self) -> DVec3 {
        self.vel - self.ground_vel()
    }

    pub fn density(&self) -> f64 {
        let h = self.alt();
        if h < ATMO_TOP { RHO0 * (-h.max(0.0) / SCALE_H).exp() } else { 0.0 }
    }

    pub fn q(&self) -> f64 {
        0.5 * self.density() * self.rel_vel().length_squared()
    }

    pub fn elements(&self) -> Elements {
        Elements::from_state(MU, self.pos, self.vel)
    }

    pub fn apo_alt(&self) -> f64 {
        self.elements().apoapsis_r - R_EARTH
    }

    pub fn peri_alt(&self) -> f64 {
        self.elements().periapsis_r - R_EARTH
    }

    pub fn mass(&self) -> f64 {
        let n = self.vessel.stages.len();
        self.vessel.stage_dry(self.stage) + self.prop
            + (self.stage + 1..n).map(|i| self.vessel.stage_dry(i) + self.vessel.stage_prop(i)).sum::<f64>()
    }

    pub fn stage_empty(&self) -> bool {
        self.prop <= 0.0
    }

    pub fn has_chute(&self) -> bool {
        (self.stage..self.vessel.stages.len())
            .any(|s| self.vessel.parts(s).any(|p| matches!(p.kind, Kind::Parachute)))
    }

    fn log(&mut self, msg: impl Into<String>) {
        self.events.push(Event { t: self.t, alt: self.alt(), msg: msg.into() });
    }

    /// Weakest non-chute part still attached.
    fn weakest_attached(&self) -> Option<(&'static str, f64)> {
        (self.stage..self.vessel.stages.len())
            .flat_map(|s| self.vessel.parts(s))
            .filter(|p| !matches!(p.kind, Kind::Parachute))
            .min_by(|a, b| a.strength_pa.total_cmp(&b.strength_pa))
            .map(|p| (p.name, p.strength_pa))
    }

    // ---- player controls ----

    pub fn set_throttle(&mut self, t: f64) {
        self.throttle = t.clamp(0.0, 1.0);
    }

    pub fn set_pitch(&mut self, deg: f64) {
        self.pitch_cmd = deg.clamp(-180.0, 180.0);
    }

    /// Fire the next stage: drops the current one and ignites the next.
    pub fn stage(&mut self) {
        if self.status.is_over() {
            return;
        }
        if self.status == Status::OnPad && self.stage == 0 && self.throttle > 0.0 {
            // First stage ignition is handled by throttle on the pad.
        }
        if self.stage + 1 >= self.vessel.stages.len() {
            self.log("Staging: nothing left to stage.");
            return;
        }
        self.stage += 1;
        self.prop = self.vessel.stage_prop(self.stage);
        self.log(format!("Stage {} separated, stage {} active", self.stage, self.stage + 1));
    }

    pub fn deploy_chute(&mut self) {
        if self.status.is_over() || self.chute_deployed {
            return;
        }
        if !self.has_chute() {
            self.log("Parachute deploy: no parachute attached.");
            return;
        }
        self.chute_deployed = true;
        self.log("Parachute deployed");
    }

    pub fn end(&mut self) {
        if !self.status.is_over() {
            self.status = Status::Ended;
            self.log("Flight ended by pilot");
        }
    }

    // ---- stepping ----

    pub fn advance(&mut self, secs: f64, dt: f64) {
        let end = self.t + secs;
        while self.t < end && !self.status.is_over() {
            self.step(dt.min(end - self.t));
        }
    }

    pub fn step(&mut self, dt: f64) {
        if self.status.is_over() || dt <= 0.0 {
            return;
        }
        // Attitude slew
        let d = self.pitch_cmd - self.pitch;
        self.pitch += d.clamp(-SLEW_DEG_S * dt, SLEW_DEG_S * dt);

        let r = self.pos.length();
        let up = self.pos / r;
        let east = DVec3::Z.cross(up).normalize();
        let p = self.pitch.to_radians();
        let dir = up * p.cos() + east * p.sin();

        let mass = self.mass();
        let (t_kn, isp_sl, isp_vac) = self.vessel.stage_engines(self.stage);
        let mut thrust = 0.0;
        if self.throttle > 0.0 && t_kn > 0.0 && self.prop > 0.0 {
            let pf = (-self.alt().max(0.0) / SCALE_H).exp();
            let isp = isp_vac + (isp_sl - isp_vac) * pf;
            thrust = t_kn * 1000.0 * self.throttle;
            let burn = thrust / (isp * G0) * dt;
            if burn >= self.prop {
                thrust *= self.prop / burn;
                self.prop = 0.0;
                self.log(format!("Stage {} out of propellant", self.stage + 1));
            } else {
                self.prop -= burn;
            }
        }

        if self.status == Status::OnPad {
            let twr = thrust / (mass * G0);
            if twr <= 1.0 {
                self.t += dt;
                if thrust > 0.0 && self.liftoff_twr == 0.0 {
                    self.log(format!("Engines lit but thrust-to-weight is {:.2}: held on the pad", twr));
                    self.liftoff_twr = -1.0;
                }
                return;
            }
            self.liftoff_twr = twr;
            self.status = Status::Flying;
            self.log(format!("Liftoff, thrust-to-weight {:.2}", twr));
        }

        // Forces
        let rho = self.density();
        let vr = self.rel_vel();
        let speed_r = vr.length();
        let mut acc = -up * MU / (r * r) + dir * thrust / mass;
        if rho > 0.0 && speed_r > 0.0 {
            let q = 0.5 * rho * speed_r * speed_r;
            let mut cda = self.vessel.cd() * REF_AREA;
            if self.chute_deployed && !self.chute_torn && self.has_chute() {
                if q > 40_000.0 {
                    self.chute_torn = true;
                    self.log(format!("Parachute torn off at {:.0} kPa", q / 1000.0));
                } else {
                    cda = CHUTE_CDA;
                }
            }
            acc -= vr / speed_r * (q * cda / mass);
        }

        self.dv_used += thrust / mass * dt;
        self.vel += acc * dt;
        self.pos += self.vel * dt;
        self.t += dt;

        // Bookkeeping
        let alt = self.alt();
        let q = self.q();
        let speed = self.vel.length();
        if q > self.max_q {
            self.max_q = q;
            self.max_q_t = self.t;
            self.max_q_alt = alt;
        }
        self.max_alt = self.max_alt.max(alt);
        self.max_speed = self.max_speed.max(self.rel_vel().length());
        if !self.reached_space && alt >= SPACE_ALT {
            self.reached_space = true;
            self.log("Crossed the Karman line (100 km): in space");
        }
        if !self.reached_orbit && self.peri_alt() >= ORBIT_PERIAPSIS && self.elements().energy < 0.0 {
            self.reached_orbit = true;
            self.log(format!("Stable orbit: {:.0} x {:.0} km", self.peri_alt() / 1e3, self.apo_alt() / 1e3));
        }
        if self.t >= self.next_sample {
            self.next_sample = self.t.floor() + 1.0;
            let el = self.elements();
            self.samples.push(Sample {
                t: self.t, alt, speed, q, mass,
                apo: el.apoapsis_r - R_EARTH, peri: el.periapsis_r - R_EARTH,
            });
        }

        // Structural failure
        if let Some((name, limit)) = self.weakest_attached() {
            if q > limit {
                self.status = Status::BrokeUp { q, limit, part: name };
                self.log(format!("Structural failure: {:.1} kPa exceeded the {} limit of {:.0} kPa", q / 1000.0, name, limit / 1000.0));
                return;
            }
        }

        // Ground contact
        if self.pos.length() <= R_EARTH && self.t > 1.0 {
            let v = self.rel_vel().length();
            self.impact_speed = Some(v);
            self.pos = up * R_EARTH;
            if v <= SAFE_LANDING {
                self.status = Status::Landed { speed: v };
                self.log(format!("Touchdown at {:.1} m/s", v));
            } else {
                self.status = Status::Crashed { speed: v };
                self.log(format!("Impact at {:.0} m/s", v));
            }
        }
    }
}

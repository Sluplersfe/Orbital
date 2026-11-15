//! Ships in space: a `Spacecraft` is a vessel that has left the atmosphere, with its propellant,
//! supplies, crew and place. Between flights the world treats it analytically: circular orbits
//! advance by a phase angle, transfers are impulsive patched-conic burns paid from the rocket
//! equation, and a transit takes designed world time (see `bodies`).
use crate::bodies::{self, Body, Route};
use crate::sim::Flight;
use crate::vessel::Vessel;
use crate::{G0, orbit};
use bevy::math::DVec3;
use std::f64::consts::TAU;

/// A circular, equatorial orbit about a body, given by radius and the angle at an epoch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    pub body: &'static str,
    pub r: f64,
    pub theta0: f64,
    pub epoch: f64,
}

impl Orbit {
    pub fn body(&self) -> &'static Body {
        bodies::body(self.body)
    }

    pub fn mean_motion(&self) -> f64 {
        (self.body().mu / self.r.powi(3)).sqrt()
    }

    pub fn period(&self) -> f64 {
        TAU / self.mean_motion()
    }

    /// Angle in the orbital plane at world time t.
    pub fn theta(&self, t: f64) -> f64 {
        (self.theta0 + self.mean_motion() * (t - self.epoch)).rem_euclid(TAU)
    }

    pub fn position(&self, t: f64) -> DVec3 {
        let th = self.theta(t);
        DVec3::new(th.cos(), th.sin(), 0.0) * self.r
    }

    pub fn velocity(&self, t: f64) -> DVec3 {
        let th = self.theta(t);
        DVec3::new(-th.sin(), th.cos(), 0.0) * self.body().circular_speed(self.r)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Place {
    Orbit(Orbit),
    /// On a body's surface at a landing site (an index; sites are not modelled further).
    Landed { body: &'static str, site: u32 },
    Docked { station: u64 },
    /// Flying between bodies; arrives at `arrive` (world seconds) in a circular orbit radius `r`.
    Transit { to: &'static str, r: f64, arrive: f64 },
    Destroyed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BurnError {
    /// Not enough delta-v: this much was still needed (m/s).
    OutOfPropellant { short: f64 },
    NoEngine,
}

impl std::fmt::Display for BurnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BurnError::OutOfPropellant { short } => write!(f, "Not enough propellant: {short:.0} m/s of delta-v short."),
            BurnError::NoEngine => write!(f, "No engine left on the vessel."),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Spacecraft {
    pub id: u64,
    pub name: String,
    pub vessel: Vessel,
    pub stage: usize,
    /// Propellant left in the active stage (kg); later stages are full.
    pub prop: f64,
    pub place: Place,
    pub crew: Vec<String>,
    pub food: f64,
    pub water: f64,
}

impl Spacecraft {
    pub fn new(id: u64, name: &str, vessel: Vessel, place: Place) -> Self {
        let (food, water) = vessel.supplies();
        let prop = vessel.stage_prop(0);
        Self { id, name: name.into(), vessel, stage: 0, prop, place, crew: vec![], food, water }
    }

    /// Mass now (kg): the active stage with its remaining propellant, plus all later stages.
    pub fn mass(&self) -> f64 {
        let n = self.vessel.stages.len();
        self.vessel.stage_dry(self.stage) + self.prop
            + (self.stage + 1..n).map(|i| self.vessel.stage_dry(i) + self.vessel.stage_prop(i)).sum::<f64>()
    }

    /// Delta-v still available in vacuum (m/s), staging as needed.
    pub fn dv_remaining(&self) -> f64 {
        let n = self.vessel.stages.len();
        let mut total = 0.0;
        for s in self.stage..n {
            let (t, _, isp) = self.vessel.stage_engines(s);
            let prop = if s == self.stage { self.prop } else { self.vessel.stage_prop(s) };
            if t == 0.0 || prop <= 0.0 {
                continue;
            }
            let above: f64 = (s + 1..n).map(|i| self.vessel.stage_dry(i) + self.vessel.stage_prop(i)).sum();
            let m0 = self.vessel.stage_dry(s) + prop + above;
            total += isp * G0 * (m0 / (m0 - prop)).ln();
        }
        total
    }

    pub fn has(&self, f: impl Fn(&crate::parts::Kind) -> bool) -> bool {
        (self.stage..self.vessel.stages.len()).any(|s| self.vessel.parts(s).any(|p| f(&p.kind)))
    }

    fn next_stage(&mut self) -> bool {
        if self.stage + 1 >= self.vessel.stages.len() {
            return false;
        }
        self.stage += 1;
        self.prop = self.vessel.stage_prop(self.stage);
        true
    }

    /// Spend `dv` m/s of delta-v with the rocket equation, dropping empty or engineless stages.
    /// Returns the burn time (s). On failure nothing is changed.
    pub fn burn(&mut self, dv: f64) -> Result<f64, BurnError> {
        let saved = (self.stage, self.prop);
        let r = self.burn_inner(dv);
        if r.is_err() {
            (self.stage, self.prop) = saved;
        }
        r
    }

    fn burn_inner(&mut self, dv: f64) -> Result<f64, BurnError> {
        let (mut rem, mut secs) = (dv, 0.0);
        while rem > 1e-9 {
            let (t_kn, _, isp) = self.vessel.stage_engines(self.stage);
            if t_kn == 0.0 || self.prop <= 0.0 {
                if !self.next_stage() {
                    return Err(if self.has(|k| matches!(k, crate::parts::Kind::Engine { .. })) {
                        BurnError::OutOfPropellant { short: rem }
                    } else {
                        BurnError::NoEngine
                    });
                }
                continue;
            }
            let m0 = self.mass();
            let ve = isp * G0;
            let mdot = t_kn * 1000.0 / ve;
            let avail = ve * (m0 / (m0 - self.prop)).ln();
            if avail >= rem {
                let used = m0 * (1.0 - (-rem / ve).exp());
                self.prop -= used;
                secs += used / mdot;
                rem = 0.0;
            } else {
                rem -= avail;
                secs += self.prop / mdot;
                self.prop = 0.0;
            }
        }
        Ok(secs)
    }

    /// Top up the active stage (a station depot or ISRU plant calls this). Returns kg added.
    pub fn refill(&mut self, limit: f64) -> f64 {
        let want = (self.vessel.stage_prop(self.stage) - self.prop).max(0.0);
        let add = want.min(limit);
        self.prop += add;
        add
    }

    /// Start a transfer: pays the whole route's delta-v now (departure and arrival burns) and
    /// sets the ship in transit. Fails without side effects if the delta-v is not there.
    pub fn start_transfer(&mut self, route: &Route, to: &'static str, r_dst: f64, now: f64) -> Result<f64, BurnError> {
        let mut secs = 0.0;
        let saved = (self.stage, self.prop);
        for leg in &route.legs {
            match self.burn(leg.dv) {
                Ok(s) => secs += s,
                Err(e) => {
                    (self.stage, self.prop) = saved;
                    return Err(e);
                }
            }
        }
        self.place = Place::Transit { to, r: r_dst, arrive: now + route.transit_s + secs };
        Ok(secs)
    }

    /// Convert the end of a launch into a ship in a circular orbit. The Ascent leaves a
    /// bound, usually slightly eccentric orbit: coast to apoapsis and circularise there, paying
    /// the delta-v. `launched_at` is the world time of liftoff.
    pub fn from_flight(id: u64, name: &str, f: &Flight, launched_at: f64) -> Result<Self, String> {
        if !f.reached_orbit {
            return Err("The flight never reached a stable orbit.".into());
        }
        let mu = f.body.mu;
        // Coast to apoapsis: the radius maximum over one period.
        let el = f.elements();
        let period = el.period(mu);
        let (mut best_t, mut best_r) = (0.0, f.pos.length());
        let n = 720;
        for i in 0..=n {
            let t = period * i as f64 / n as f64;
            let r = orbit::propagate(mu, f.pos, f.vel, t).0.length();
            if r > best_r {
                (best_t, best_r) = (t, r);
            }
        }
        let (p_apo, v_apo) = orbit::propagate(mu, f.pos, f.vel, best_t);
        let dv_circ = (mu / p_apo.length()).sqrt() - v_apo.length();
        let mut sc = Self::new(id, name, f.vessel.clone(), Place::Destroyed);
        sc.stage = f.stage;
        sc.prop = f.prop;
        sc.burn(dv_circ.abs()).map_err(|e| format!("Could not circularise: {e}"))?;
        sc.place = Place::Orbit(Orbit {
            body: f.body.id,
            r: p_apo.length(),
            theta0: p_apo.y.atan2(p_apo.x).rem_euclid(TAU),
            epoch: launched_at + f.t + best_t,
        });
        Ok(sc)
    }

    /// Start a `Flight` from this ship's orbit at world time `now` (for a descent or a burn).
    pub fn flight_in_orbit(&self, now: f64) -> Option<Flight> {
        let Place::Orbit(o) = self.place else { return None };
        Some(Flight::in_space(o.body(), self.vessel.clone(), self.stage, self.prop, o.position(now), o.velocity(now)))
    }
}

pub fn route_to(sc: &Spacecraft, dst: &str) -> Option<(Route, &'static str, f64)> {
    let Place::Orbit(o) = sc.place else { return None };
    let d = bodies::get(dst)?;
    let r_dst = d.park_radius();
    Some((bodies::route(o.body, o.r, dst, r_dst), d.id, r_dst))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bodies::earth;

    fn ship(stages: &[&[&'static str]]) -> Spacecraft {
        let v = Vessel { stages: stages.iter().map(|s| s.to_vec()).collect() };
        Spacecraft::new(1, "t", v, Place::Orbit(Orbit { body: "earth", r: earth().park_radius(), theta0: 0.0, epoch: 0.0 }))
    }

    #[test]
    fn burn_matches_the_rocket_equation_and_stages() {
        let mut s = ship(&[&["vector", "tank_l"], &["vacuum", "tank_m", "pod1"]]);
        let total = s.dv_remaining();
        assert!(total > 3_000.0, "{total}");
        // Burn part of the first stage.
        let m0 = s.mass();
        s.burn(500.0).unwrap();
        let ve = 315.0 * G0;
        let want_used = m0 * (1.0 - (-500.0 / ve).exp());
        assert!((s.vessel.stage_prop(0) - s.prop - want_used).abs() < 1.0);
        assert_eq!(s.stage, 0);
        // Burn through it: the dry first stage is dropped and the vacuum stage takes over.
        s.burn(total - 600.0).unwrap();
        assert_eq!(s.stage, 1);
        assert!((s.dv_remaining() - 100.0).abs() < 5.0, "{}", s.dv_remaining());
    }

    #[test]
    fn failed_burn_changes_nothing() {
        let mut s = ship(&[&["vacuum", "tank_s", "pod1"]]);
        let before = (s.stage, s.prop);
        let e = s.burn(99_999.0).unwrap_err();
        assert!(matches!(e, BurnError::OutOfPropellant { short } if short > 90_000.0));
        assert_eq!((s.stage, s.prop), before);
        let mut dead = ship(&[&["tank_s", "pod1"]]);
        assert_eq!(dead.burn(10.0), Err(BurnError::NoEngine));
    }

    #[test]
    fn transfer_pays_delta_v_and_takes_designed_time() {
        let mut s = ship(&[&["vector", "tank_xl", "tank_xl"], &["vacuum", "tank_l", "pod1"]]);
        let (route, to, r) = route_to(&s, "moon").unwrap();
        let before = s.dv_remaining();
        s.start_transfer(&route, to, r, 1000.0).unwrap();
        assert!((before - s.dv_remaining() - route.total_dv).abs() < 20.0);
        assert!(matches!(s.place, Place::Transit { to: "moon", arrive, .. } if arrive > 1000.0 + 900.0));
        // Too little delta-v for Mars: refused and unchanged.
        let mut small = ship(&[&["vacuum", "tank_m", "pod1"]]);
        let (route, to, r) = route_to(&small, "mars").unwrap();
        let (st, pr) = (small.stage, small.prop);
        assert!(small.start_transfer(&route, to, r, 0.0).is_err());
        assert_eq!((small.stage, small.prop), (st, pr));
        assert!(matches!(small.place, Place::Orbit(_)));
    }

    #[test]
    fn orbit_phase_advances_with_the_mean_motion() {
        let o = Orbit { body: "earth", r: earth().park_radius(), theta0: 1.0, epoch: 100.0 };
        let t = o.period();
        assert!((o.theta(100.0 + t) - 1.0).abs() < 1e-9);
        assert!((o.velocity(5.0).length() - earth().circular_speed(o.r)).abs() < 1e-6);
    }
}

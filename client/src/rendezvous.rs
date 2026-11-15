//! Rendezvous and docking between two ships in circular, coplanar orbits about one body.
//!
//! 1. `plan_intercept`: a Hohmann transfer timed so the chaser arrives in the target's orbit a
//!    standoff distance behind it (or, for equal altitudes, a phasing orbit).
//! 2. `final_approach`: a two-impulse Clohessy-Wiltshire transfer from the standoff point to the
//!    docking port, checked against the real two-body motion (`approach_miss`).
use crate::orbit::propagate;
use crate::space::Orbit;
use bevy::math::{DVec2, DVec3};
use std::f64::consts::{PI, TAU};

/// Distance behind the target where the intercept ends and the final approach starts (m).
pub const STANDOFF: f64 = 3_000.0;
/// A docking port captures within this distance (m) and relative speed (m/s).
pub const DOCK_RANGE: f64 = 20.0;
pub const DOCK_SPEED: f64 = 0.5;
/// Longest we will wait for the transfer window before parking in a drift orbit first (s).
const MAX_WAIT_S: f64 = 6.0 * 3600.0;
/// Most delta-v a drift orbit may cost (both burns, m/s).
const MAX_DRIFT_DV: f64 = 300.0;
/// Final approach duration, as a fraction of the orbital period.
const APPROACH_FRACTION: f64 = 0.25;

#[derive(Debug, Clone, PartialEq)]
pub struct Intercept {
    /// (world time, delta-v magnitude) of each burn.
    pub burns: Vec<(f64, f64)>,
    /// World time the chaser is in the target orbit, `STANDOFF` behind.
    pub arrive: f64,
    pub dv: f64,
    /// Orbit the chaser is left in after the last burn.
    pub result: Orbit,
}

fn wrap(a: f64) -> f64 {
    a.rem_euclid(TAU)
}

/// Plan the burns that put `chaser` `STANDOFF` behind `target`. Both must orbit the same body.
pub fn plan_intercept(chaser: &Orbit, target: &Orbit, now: f64) -> Result<Intercept, String> {
    if chaser.body != target.body {
        return Err(format!("Not in the same sphere of influence ({} and {}).", chaser.body, target.body));
    }
    let body = chaser.body();
    let mu = body.mu;
    let (rc, rt) = (chaser.r, target.r);
    let (nc, nt) = (chaser.mean_motion(), target.mean_motion());
    let lag = STANDOFF / rt;
    // Phase of the target ahead of the chaser, now.
    let phi0 = wrap(target.theta(now) - chaser.theta(now));
    if (rc - rt).abs() > 2_000.0 {
        let at = (rc + rt) / 2.0;
        let t_tr = PI * (at.powi(3) / mu).sqrt();
        let want = wrap(PI - lag - nt * t_tr); // target must lead the chaser by this at departure
        let rate = nt - nc;
        let mut wait = (want - phi0) / rate;
        let period = TAU / rate.abs();
        while wait < 0.0 {
            wait += period;
        }
        while wait >= period {
            wait -= period;
        }
        // Close orbits drift apart so slowly that waiting for the phase can take months: park in
        // a faster or slower orbit for a few revolutions first.
        let mut pre: Vec<(f64, f64)> = vec![];
        if wait > MAX_WAIT_S {
            if let Some((tp_wait, burns)) = drift_plan(chaser, target, now, want, phi0).filter(|d| d.0 < wait) {
                wait = tp_wait;
                pre = burns;
            }
        }
        let (dep, arrive) = (now + wait, now + wait + t_tr);
        let (v_c1, v_c2) = ((mu / rc).sqrt(), (mu / rt).sqrt());
        let v_p = (mu * (2.0 / rc - 1.0 / at)).sqrt();
        let v_a = (mu * (2.0 / rt - 1.0 / at)).sqrt();
        let (d1, d2) = ((v_p - v_c1).abs(), (v_c2 - v_a).abs());
        let result = Orbit { body: chaser.body, r: rt, theta0: wrap(target.theta(arrive) - lag), epoch: arrive };
        let mut burns = pre;
        burns.push((dep, d1));
        burns.push((arrive, d2));
        let dv = burns.iter().map(|b| b.1).sum();
        return Ok(Intercept { burns, arrive, dv, result });
    }
    // Same altitude: a phasing orbit. The chaser must gain `gain` radians on the target.
    let gain = wrap(phi0 - lag);
    let period = TAU / nt;
    let min_peri = body.radius + body.min_orbit_alt();
    for k in 1..=40 {
        let tp = period * (1.0 - gain / (TAU * k as f64));
        let a_p = (mu * (tp / TAU).powi(2)).cbrt();
        let peri = 2.0 * a_p - rt;
        if peri < min_peri && gain > 1e-6 {
            continue;
        }
        let v_circ = (mu / rt).sqrt();
        let v_apo = (mu * (2.0 / rt - 1.0 / a_p)).sqrt();
        let d = (v_circ - v_apo).abs();
        let end = now + tp * k as f64;
        let result = Orbit { body: chaser.body, r: rt, theta0: wrap(target.theta(end) - lag), epoch: end };
        return Ok(Intercept { burns: vec![(now, d), (end, d)], arrive: end, dv: 2.0 * d, result });
    }
    Err("No safe phasing orbit: the target is too close behind. Raise or lower your orbit first.".into())
}

/// Phase the chaser with a drift orbit so it is back in its own circular orbit, `want` radians
/// behind the target, after `k` revolutions. Returns (wait until the window, the two burns).
fn drift_plan(chaser: &Orbit, target: &Orbit, now: f64, want: f64, phi0: f64) -> Option<(f64, Vec<(f64, f64)>)> {
    let body = chaser.body();
    let mu = body.mu;
    let (rc, nt) = (chaser.r, target.mean_motion());
    let tc = chaser.period();
    let min_peri = body.radius + body.min_orbit_alt();
    let mut best: Option<(f64, f64, f64)> = None; // (duration, tp, k)
    for k in 1..=30 {
        let kf = k as f64;
        let m0 = ((nt * kf * tc - (want - phi0)) / TAU).round();
        for dm in [-1.0, 0.0, 1.0] {
            let tp = (want - phi0 + TAU * (m0 + dm)) / (nt * kf);
            if tp < 0.6 * tc || tp > 2.0 * tc {
                continue;
            }
            let a_p = (mu * (tp / TAU).powi(2)).cbrt();
            if 2.0 * a_p - rc < min_peri {
                continue;
            }
            let dur = tp * kf;
            let d = ((mu / rc).sqrt() - (mu * (2.0 / rc - 1.0 / a_p)).sqrt()).abs();
            if 2.0 * d > MAX_DRIFT_DV {
                continue;
            }
            if best.map_or(true, |b| dur < b.0) {
                best = Some((dur, tp, kf));
            }
        }
    }
    let (dur, tp, _) = best?;
    let a_p = (mu * (tp / TAU).powi(2)).cbrt();
    let d = ((mu / rc).sqrt() - (mu * (2.0 / rc - 1.0 / a_p)).sqrt()).abs();
    Some((dur, vec![(now, d), (now + dur, d)]))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Approach {
    /// First burn in the target's local frame (x radial out, y along-track), m/s.
    pub dv1: DVec2,
    pub dv2: DVec2,
    pub duration: f64,
    pub dv: f64,
}

/// Two-impulse Clohessy-Wiltshire transfer from `STANDOFF` behind the target, at rest relative to
/// it, to the target (zero relative position and velocity), over a fraction of an orbit.
pub fn final_approach(target: &Orbit) -> Approach {
    let n = target.mean_motion();
    let t = target.period() * APPROACH_FRACTION;
    let (s, c) = ((n * t).sin(), (n * t).cos());
    let (x0, y0) = (0.0, -STANDOFF);
    // Position at T: Phi_rr * r0 + Phi_rv * v0 = 0.
    let rr_x = (4.0 - 3.0 * c) * x0;
    let rr_y = 6.0 * (s - n * t) * x0 + y0;
    let (a, b) = (s / n, 2.0 * (1.0 - c) / n);
    let (cc, d) = (-2.0 * (1.0 - c) / n, (4.0 * s - 3.0 * n * t) / n);
    let det = a * d - b * cc;
    let (px, py) = (-rr_x, -rr_y);
    let v0 = DVec2::new((d * px - b * py) / det, (a * py - cc * px) / det);
    // Velocity at T from the transition matrix.
    let vx = 3.0 * n * s * x0 + c * v0.x + 2.0 * s * v0.y;
    let vy = 6.0 * n * (c - 1.0) * x0 - 2.0 * s * v0.x + (4.0 * c - 3.0) * v0.y;
    let dv2 = DVec2::new(-vx, -vy);
    Approach { dv1: v0, dv2, duration: t, dv: v0.length() + dv2.length() }
}

/// Fly `final_approach` through the real two-body motion and return (miss distance m, relative
/// speed m/s) at the end. This is what decides whether the ports meet.
pub fn approach_miss(target: &Orbit, ap: &Approach) -> (f64, f64) {
    let mu = target.body().mu;
    let t0 = target.epoch;
    let frame = |o: &Orbit, t: f64| {
        let (p, v) = (o.position(t), o.velocity(t));
        let up = p.normalize();
        let along = DVec3::Z.cross(up);
        (p, v, up, along)
    };
    let (_, _, up, along) = frame(target, t0);
    // Place the chaser exactly STANDOFF of arc behind: same radius, angle lag.
    let lag = STANDOFF / target.r;
    let th = target.theta(t0) - lag;
    let cp = DVec3::new(th.cos(), th.sin(), 0.0) * target.r;
    let cv = DVec3::new(-th.sin(), th.cos(), 0.0) * target.body().circular_speed(target.r);
    // Burn 1 in the target's local frame at t0 (local axes are those of the target).
    let burn = |d: DVec2, up: DVec3, along: DVec3| up * d.x + along * d.y;
    let cv1 = cv + burn(ap.dv1, up, along);
    let (cp2, cv2) = propagate(mu, cp, cv1, ap.duration);
    let t1 = t0 + ap.duration;
    let (tp2, tv2, up2, along2) = frame(target, t1);
    let cv3 = cv2 + burn(ap.dv2, up2, along2);
    ((cp2 - tp2).length(), (cv3 - tv2).length())
}

pub fn can_dock(miss: f64, speed: f64) -> bool {
    miss <= DOCK_RANGE && speed <= DOCK_SPEED
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bodies::earth;

    fn orb(alt: f64, theta: f64) -> Orbit {
        Orbit { body: "earth", r: earth().radius + alt, theta0: theta, epoch: 0.0 }
    }

    #[test]
    fn hohmann_intercept_ends_behind_the_target() {
        for (theta_c, theta_t) in [(0.0, 2.0), (3.0, 0.5), (5.9, 5.8), (1.0, 1.0)] {
            let chaser = orb(190_000.0, theta_c);
            let target = orb(400_000.0, theta_t);
            let plan = plan_intercept(&chaser, &target, 50.0).unwrap();
            assert!(plan.dv > 100.0 && plan.dv < 700.0, "{}", plan.dv);
            let gap = wrap(target.theta(plan.arrive) - plan.result.theta(plan.arrive));
            assert!((gap * target.r - STANDOFF).abs() < 1.0, "{gap}");
            assert!(plan.arrive >= 50.0 && plan.arrive - 50.0 < 2.0 * TAU / (chaser.mean_motion() - target.mean_motion()).abs());
        }
    }

    #[test]
    fn close_orbits_do_not_take_months() {
        for theta_t in [0.2, 1.5, 3.0, 4.4, 6.0] {
            let chaser = orb(139_000.0, 0.0);
            let target = orb(135_000.0, theta_t);
            let plan = plan_intercept(&chaser, &target, 0.0).unwrap();
            assert!(plan.arrive < 40.0 * 3600.0, "{theta_t}: {} h", plan.arrive / 3600.0);
            assert!(plan.dv < 400.0, "{}", plan.dv);
            let gap = wrap(target.theta(plan.arrive) - plan.result.theta(plan.arrive));
            assert!((gap * target.r - STANDOFF).abs() < 1.0, "{gap}");
        }
    }

    #[test]
    fn equal_altitude_uses_a_phasing_orbit() {
        let chaser = orb(400_000.0, 0.0);
        let target = orb(400_000.0, 1.5);
        let plan = plan_intercept(&chaser, &target, 0.0).unwrap();
        assert!(plan.dv < 400.0, "{}", plan.dv);
        let gap = wrap(target.theta(plan.arrive) - plan.result.theta(plan.arrive));
        assert!((gap * target.r - STANDOFF).abs() < 1.0);
    }

    #[test]
    fn final_approach_docks_in_the_real_two_body_motion() {
        let target = orb(400_000.0, 0.7);
        let ap = final_approach(&target);
        assert!(ap.dv < 10.0, "approach costs {} m/s", ap.dv);
        let (miss, speed) = approach_miss(&target, &ap);
        assert!(can_dock(miss, speed), "miss {miss:.2} m, speed {speed:.3} m/s");
    }

    #[test]
    fn a_bad_approach_misses() {
        let target = orb(400_000.0, 0.7);
        let mut ap = final_approach(&target);
        ap.dv1.y *= 1.2; // 20% too fast
        let (miss, _) = approach_miss(&target, &ap);
        assert!(miss > DOCK_RANGE, "{miss}");
    }

    #[test]
    fn different_bodies_cannot_rendezvous() {
        let a = orb(400_000.0, 0.0);
        let b = Orbit { body: "moon", r: 1.8e6, theta0: 0.0, epoch: 0.0 };
        assert!(plan_intercept(&a, &b, 0.0).is_err());
    }
}

//! Keplerian orbit math. Must match the Go module via shared test vectors.
use bevy::math::DVec3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Elements {
    pub a: f64,
    pub e: f64,
    /// Specific orbital energy; < 0 means bound.
    pub energy: f64,
    pub periapsis_r: f64,
    pub apoapsis_r: f64,
}

impl Elements {
    pub fn from_state(mu: f64, r: DVec3, v: DVec3) -> Self {
        let rn = r.length();
        let energy = v.length_squared() / 2.0 - mu / rn;
        let h = r.cross(v);
        let evec = v.cross(h) / mu - r / rn;
        let e = evec.length();
        if energy >= 0.0 {
            let p = h.length_squared() / mu;
            return Self { a: f64::INFINITY, e, energy, periapsis_r: p / (1.0 + e), apoapsis_r: f64::INFINITY };
        }
        let a = -mu / (2.0 * energy);
        Self { a, e, energy, periapsis_r: a * (1.0 - e), apoapsis_r: a * (1.0 + e) }
    }

    pub fn period(&self, mu: f64) -> f64 {
        std::f64::consts::TAU * (self.a.powi(3) / mu).sqrt()
    }
}

/// Solve Kepler's equation M = E - e sin E for E (elliptic).
pub fn solve_kepler(m: f64, e: f64) -> f64 {
    let m = m.rem_euclid(std::f64::consts::TAU);
    let mut ea = if e < 0.8 { m } else { std::f64::consts::PI };
    for _ in 0..50 {
        let d = (ea - e * ea.sin() - m) / (1.0 - e * ea.cos());
        ea -= d;
        if d.abs() < 1e-13 {
            break;
        }
    }
    ea
}

/// Circular orbital speed at radius r.
pub fn circular_speed(mu: f64, r: f64) -> f64 {
    (mu / r).sqrt()
}

/// Advance a bound two-body state by dt seconds (f and g functions in the change of eccentric
/// anomaly). This is how on-rails ships move; it mirrors orbit.Propagate in Go.
pub fn propagate(mu: f64, r0: DVec3, v0: DVec3, dt: f64) -> (DVec3, DVec3) {
    let rn = r0.length();
    let a = 1.0 / (2.0 / rn - v0.length_squared() / mu);
    let sqa = a.sqrt();
    let sig = r0.dot(v0) / mu.sqrt();
    let n = (mu / (a * a * a)).sqrt();
    let m = (n * dt) % std::f64::consts::TAU;
    let mut de = m;
    for _ in 0..60 {
        let (s, c) = de.sin_cos();
        let f = de + sig / sqa * (1.0 - c) - (1.0 - rn / a) * s - m;
        let d = f / (1.0 + sig / sqa * s - (1.0 - rn / a) * c);
        de -= d;
        if d.abs() < 1e-14 {
            break;
        }
    }
    let (s, c) = de.sin_cos();
    let r = a + (rn - a) * c + sig * sqa * s;
    let f = 1.0 - a / rn * (1.0 - c);
    let g = m / n - (a * a * a / mu).sqrt() * (de - s);
    let fd = -(mu * a).sqrt() / (r * rn) * s;
    let gd = 1.0 - a / r * (1.0 - c);
    (r0 * f + v0 * g, r0 * fd + v0 * gd)
}

#[cfg(test)]
mod tests {
    #[test]
    fn propagate_shared_vector() {
        // Shared with server/internal/orbit: r0=(7e6,0,0), v0=(0,7800,1000), dt=1234 s.
        let (r, _) = propagate(MU, DVec3::new(7e6, 0.0, 0.0), DVec3::new(0.0, 7800.0, 1000.0), 1234.0);
        let want = DVec3::new(1800978.2209196573, 7167400.1039350107, 918897.44922243722);
        assert!((r - want).length() < 1e-4, "{r:?}");
    }

    use super::*;
    use crate::{MU, R_EARTH};

    #[test]
    fn circular() {
        let r = R_EARTH + 200_000.0;
        let el = Elements::from_state(MU, DVec3::new(r, 0.0, 0.0), DVec3::new(0.0, circular_speed(MU, r), 0.0));
        assert!((el.a - r).abs() < 1e-3 && el.e < 1e-9);
        assert!((el.period(MU) - 5301.0).abs() < 5.0, "{}", el.period(MU));
    }

    #[test]
    fn kepler() {
        for &(m, e) in &[(1.0, 0.1), (3.0, 0.7), (0.2, 0.95)] {
            let ea = solve_kepler(m, e);
            assert!((ea - e * ea.sin() - m).abs() < 1e-11);
        }
        // Test vector shared with the Go module: M=1.0, e=0.5 -> E=1.4987011335178482
        assert!((solve_kepler(1.0, 0.5) - 1.4987011335178482).abs() < 1e-12);
    }
}

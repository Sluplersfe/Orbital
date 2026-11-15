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

#[cfg(test)]
mod tests {
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

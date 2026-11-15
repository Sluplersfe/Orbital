//! Research points, milestones, unlocks and eras (1: suborbital, 2: orbit).
use crate::parts::{self, CATALOG};
use crate::report::{FlightReport, Outcome};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub rp: u32,
    pub unlocked: BTreeSet<String>,
    pub milestones: BTreeSet<&'static str>,
}

pub const ERA_NAMES: [&str; 2] = ["Suborbital", "Orbit"];

impl Progress {
    pub fn new() -> Self {
        let unlocked = CATALOG.iter().filter(|p| p.cost_rp == 0).map(|p| p.id.to_string()).collect();
        Self { rp: 0, unlocked, milestones: BTreeSet::new() }
    }

    /// Current era: 2 once space has been reached and the vessel recovered.
    pub fn era(&self) -> u8 {
        if self.milestones.contains("space_recovered") || self.milestones.contains("orbit") { 2 } else { 1 }
    }

    pub fn has(&self, id: &str) -> bool {
        self.unlocked.contains(id)
    }

    pub fn can_unlock(&self, id: &str) -> bool {
        parts::get(id).is_some_and(|p| !self.has(id) && p.era <= self.era() && self.rp >= p.cost_rp)
    }

    pub fn unlock(&mut self, id: &str) -> bool {
        if !self.can_unlock(id) {
            return false;
        }
        self.rp -= parts::get(id).unwrap().cost_rp;
        self.unlocked.insert(id.to_string());
        true
    }

    /// Award research for a finished flight. Every flight returns something;
    /// first-time milestones pay most. Returns (points, lines explaining them).
    pub fn award(&mut self, r: &FlightReport) -> (u32, Vec<String>) {
        let mut pts = 0;
        let mut why = vec![];
        let mut add = |p: &mut Progress, key: &'static str, n: u32, label: &str| {
            if p.milestones.insert(key) {
                pts += n;
                why.push(format!("+{n} first {label}"));
            }
        };
        if r.max_alt >= 1_000.0 { add(self, "liftoff", 5, "liftoff"); }
        if r.max_alt >= 30_000.0 { add(self, "high_alt", 10, "flight above 30 km"); }
        if r.max_alt >= crate::SPACE_ALT { add(self, "space", 25, "trip to space"); }
        if matches!(r.outcome, Outcome::SpaceRecovered | Outcome::Orbit) && r.duration > 0.0 {
            if r.outcome == Outcome::SpaceRecovered { add(self, "space_recovered", 30, "space flight recovered"); }
        }
        if r.outcome == Outcome::Orbit { add(self, "orbit", 100, "stable orbit"); }
        // Telemetry is worth something even from a failure.
        let data = match r.outcome {
            Outcome::BrokeUp | Outcome::Crashed | Outcome::Suborbital | Outcome::SpaceLost => 4 + (r.max_alt / 20_000.0) as u32,
            _ => 3,
        };
        if r.outcome != Outcome::NeverLaunched && r.outcome != Outcome::Aborted {
            pts += data;
            why.push(format!("+{data} flight data"));
        }
        self.rp += pts;
        (pts, why)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_unlocks_and_costs() {
        let p = Progress::new();
        assert!(p.has("spark") && !p.has("vector"));
        assert!(!p.can_unlock("vector")); // era 2 and no points
    }
}

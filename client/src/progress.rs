//! Research points, milestones, claims, unlocks and eras (1 suborbital, 2 orbit, 3 stations,
//! 4 Moon and asteroids, 5 planets).
use crate::parts;
use crate::report::{FlightReport, Outcome};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub rp: u32,
    pub unlocked: BTreeSet<String>,
    pub milestones: BTreeSet<&'static str>,
    /// Resources claimed by being there. Shared by every player; a claim unlocks parts.
    pub claims: BTreeSet<String>,
}

pub const ERA_NAMES: [&str; 5] = ["Suborbital", "Orbit", "Stations", "Moon and asteroids", "Planets"];

impl Progress {
    pub fn new() -> Self {
        let unlocked = parts::catalog().iter().filter(|p| p.cost_rp == 0 && p.requires.is_empty() && p.claim.is_none())
            .map(|p| p.id.to_string()).collect();
        Self { rp: 0, unlocked, milestones: BTreeSet::new(), claims: BTreeSet::new() }
    }

    /// Current era. Each era's goal opens the next: recover from space (2), reach orbit (3),
    /// build a station (4), claim a first resource off Earth (5).
    pub fn era(&self) -> u8 {
        if !self.claims.is_empty() {
            5
        } else if self.milestones.contains("station") {
            4
        } else if self.milestones.contains("orbit") {
            3
        } else if self.milestones.contains("space_recovered") {
            2
        } else {
            1
        }
    }

    pub fn has(&self, id: &str) -> bool {
        self.unlocked.contains(id)
    }

    /// Why a part cannot be unlocked right now, or None if it can.
    pub fn blocker(&self, id: &str) -> Option<String> {
        let p = parts::get(id)?;
        if self.has(id) {
            return Some(format!("{} is already unlocked.", p.name));
        }
        if p.era > self.era() {
            return Some(format!("{} belongs to era {} ({}): finish the era {} goal first.", p.name, p.era, ERA_NAMES[p.era as usize - 1], self.era()));
        }
        if let Some(r) = p.requires.iter().find(|r| !self.has(r)) {
            return Some(format!("{} needs {} unlocked first.", p.name, parts::get(r).map_or(*r, |q| q.name)));
        }
        if let Some(c) = p.claim {
            if !self.claims.contains(c) {
                return Some(format!("{} needs the {} resource claimed: send a crew to survey it.", p.name, c));
            }
        }
        if self.rp < p.cost_rp {
            return Some(format!("Need {} research points ({} short).", p.cost_rp, p.cost_rp - self.rp));
        }
        None
    }

    pub fn can_unlock(&self, id: &str) -> bool {
        parts::get(id).is_some() && self.blocker(id).is_none()
    }

    pub fn unlock(&mut self, id: &str) -> bool {
        if !self.can_unlock(id) {
            return false;
        }
        self.rp -= parts::get(id).unwrap().cost_rp;
        self.unlocked.insert(id.to_string());
        true
    }

    /// One-off research award for a named milestone. Returns the line explaining it, if new.
    pub fn grant(&mut self, key: &'static str, pts: u32, label: &str) -> Option<String> {
        if !self.milestones.insert(key) {
            return None;
        }
        self.rp += pts;
        Some(format!("+{pts} first {label}"))
    }

    /// Record claims; returns the ones that were new.
    pub fn claim_all(&mut self, resources: &[&str]) -> Vec<String> {
        resources.iter().filter(|r| self.claims.insert(r.to_string())).map(|r| r.to_string()).collect()
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

    #[test]
    fn unlocks_need_era_prereqs_claims_and_points() {
        let mut p = Progress::new();
        p.rp = 10_000;
        assert!(p.blocker("vector").unwrap().contains("era 2"));
        p.milestones.insert("space_recovered");
        assert!(p.unlock("vector"));
        // Era 3 part with a prerequisite.
        p.milestones.insert("orbit");
        assert!(p.blocker("core1").unwrap().contains("Docking Port"));
        assert!(p.unlock("dock1") && p.unlock("core1"));
        // Claim-gated part.
        p.milestones.insert("station");
        p.unlock("hab1");
        p.unlock("depot1");
        assert!(p.blocker("isru1").unwrap().contains("water_ice"));
        p.claim_all(&["water_ice"]);
        assert_eq!(p.era(), 5);
        assert!(p.unlock("isru1"));
    }
}

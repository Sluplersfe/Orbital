//! Flight reports: every flight, especially failures, explains itself.
use crate::sim::{Flight, Status};
use crate::vessel::orbit_dv_budget;
use crate::{R_EARTH, SPACE_ALT};

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Orbit,
    SpaceRecovered,
    SpaceLost,
    Suborbital,
    BrokeUp,
    Crashed,
    NeverLaunched,
    Aborted,
    /// Landed on another body, or came down from orbit safely.
    Landed,
}

#[derive(Debug, Clone)]
pub struct FlightReport {
    pub outcome: Outcome,
    pub headline: String,
    /// Why it went the way it did, most important first.
    pub diagnosis: Vec<String>,
    pub timeline: Vec<String>,
    pub max_alt: f64,
    pub max_speed: f64,
    pub max_q: f64,
    pub dv_used: f64,
    pub dv_design: f64,
    pub duration: f64,
    pub orbit: Option<(f64, f64)>,
}

impl FlightReport {
    pub fn from_flight(f: &Flight) -> Self {
        if f.body.id != "earth" || f.from_orbit {
            return Self::lander_report(f);
        }
        let dv_design = f.vessel.total_dv();
        let mut d: Vec<String> = vec![];
        let orbit = f.reached_orbit.then(|| (f.peri_alt(), f.apo_alt()));
        let bound = f.elements().energy < 0.0;

        let outcome = match f.status {
            Status::OnPad => Outcome::NeverLaunched,
            _ if f.liftoff_twr <= 0.0 => Outcome::NeverLaunched,
            Status::BrokeUp { .. } => Outcome::BrokeUp,
            Status::Crashed { .. } => Outcome::Crashed,
            Status::Landed { .. } if f.reached_orbit => Outcome::Orbit,
            Status::Landed { .. } if f.reached_space => Outcome::SpaceRecovered,
            Status::Landed { .. } => Outcome::Suborbital,
            _ if f.reached_orbit => Outcome::Orbit,
            _ if f.reached_space => Outcome::SpaceLost,
            _ if f.status == Status::Ended && f.max_alt < 1.0 => Outcome::Aborted,
            _ => Outcome::Suborbital,
        };

        let headline = match (&outcome, f.status) {
            (Outcome::Orbit, _) => {
                let (p, a) = orbit.unwrap_or((f.peri_alt(), f.apo_alt()));
                format!("Orbit achieved: {:.0} x {:.0} km", p / 1e3, a / 1e3)
            }
            (Outcome::SpaceRecovered, _) => format!("Reached space ({:.0} km) and recovered the vessel", f.max_alt / 1e3),
            (Outcome::SpaceLost, _) => format!("Reached space ({:.0} km), vessel not recovered", f.max_alt / 1e3),
            (Outcome::Suborbital, _) => format!("Suborbital hop, peak {:.1} km", f.max_alt / 1e3),
            (Outcome::BrokeUp, Status::BrokeUp { part, .. }) => format!("Vessel broke apart: {} failed at max-Q", part),
            (Outcome::Crashed, Status::Crashed { speed, .. }) => format!("Crashed at {:.0} m/s", speed),
            (Outcome::NeverLaunched, _) => "Never left the pad".into(),
            _ => "Flight aborted".into(),
        };

        // ---- diagnosis ----
        match f.status {
            _ if outcome == Outcome::NeverLaunched => {
                if f.liftoff_twr < 0.0 {
                    let s = f.vessel.stats();
                    d.push(format!(
                        "Liftoff thrust-to-weight was {:.2}; you need more than 1.0 (about 1.3-1.6 is comfortable). Add engines or remove mass.",
                        s.first().map_or(0.0, |s| s.twr_sl)
                    ));
                } else {
                    d.push("Throttle was never opened. Press Z/X (or Shift) to raise throttle, then Space to launch.".into());
                }
            }
            Status::BrokeUp { q, limit, part } => {
                d.push(format!(
                    "Max-Q: air pressure hit {:.1} kPa at {:.0} km, {:.0} s into the flight. {} only survives {:.0} kPa.",
                    q / 1e3, f.alt() / 1e3, f.t, part, limit / 1e3
                ));
                d.push("Fixes: throttle down through the thickest air (about 8-14 km), use a Nose Cone to cut drag, or add a Reinforced Ring / swap in a stronger part.".into());
                if f.pitch < 5.0 && f.alt() > 5_000.0 {
                    d.push("You flew straight up. Pitching over gradually trades air speed for horizontal speed while the air is thin.".into());
                }
            }
            Status::Crashed { speed } => {
                if !f.has_chute() && !f.chute_torn {
                    d.push("No parachute was left on the vessel (not fitted, or staged away).".into());
                } else if f.chute_torn {
                    d.push("The parachute tore off: it was deployed above 40 kPa. Deploy lower and slower.".into());
                } else if !f.chute_deployed {
                    d.push("The parachute was never deployed. Press P once you are below ~10 km and slower than ~300 m/s.".into());
                } else {
                    d.push(format!("The parachute was open but the impact speed was still {:.0} m/s (safe is under {:.0}). It opened too late.", speed, crate::sim::SAFE_LANDING));
                }
                if let Some(e) = f.events.iter().find(|e| e.msg.contains("out of propellant")) {
                    d.push(format!("The last stage ran dry at {:.1} km, {:.0} s in.", e.alt / 1e3, e.t));
                }
            }
            _ => {}
        }

        if !f.reached_orbit && outcome != Outcome::NeverLaunched && !matches!(f.status, Status::BrokeUp { .. }) {
            let need = orbit_dv_budget();
            if dv_design < need {
                d.push(format!(
                    "Design delta-v is {:.0} m/s but reaching orbit costs about {:.0} m/s (orbital speed plus gravity and drag losses). The builder's delta-v readout shows this before launch: add stages or propellant.",
                    dv_design, need
                ));
            } else if f.dv_used < dv_design * 0.9 && f.stage_empty() {
                d.push("Delta-v on paper was enough, but not all of it was used on a useful heading.".into());
            }
            if f.reached_space || f.max_alt > 30_000.0 {
                if !bound || f.peri_alt() < 0.0 {
                    d.push(format!(
                        "Orbit needs speed sideways, not just height. At the highest point the periapsis was {:.0} km (below the ground), so the path came back down. Pitch toward the horizon (90 degrees) earlier and burn along it.",
                        f.peri_alt() / 1e3
                    ));
                } else if f.peri_alt() < 80_000.0 {
                    d.push(format!(
                        "Periapsis {:.0} km is still inside the atmosphere (needs > 80 km). Burn prograde at apoapsis to raise it.",
                        f.peri_alt() / 1e3
                    ));
                }
            }
            if f.max_alt < SPACE_ALT && outcome != Outcome::NeverLaunched {
                d.push(format!("Peak altitude {:.1} km; space starts at 100 km.", f.max_alt / 1e3));
            }
        }
        if f.liftoff_twr > 0.0 && f.liftoff_twr < 1.2 {
            d.push(format!("Liftoff thrust-to-weight was only {:.2}. Low TWR wastes delta-v fighting gravity.", f.liftoff_twr));
        }
        if matches!(outcome, Outcome::Orbit) {
            d.push(format!(
                "Used {:.0} m/s of {:.0} m/s design delta-v ({:.0} m/s lost to gravity and drag relative to a {:.0} m/s circular orbit).",
                f.dv_used, dv_design, (f.dv_used - (crate::MU / (R_EARTH + f.peri_alt().max(0.0))).sqrt()).max(0.0),
                (crate::MU / (R_EARTH + f.peri_alt().max(0.0))).sqrt()
            ));
        }
        if d.is_empty() {
            d.push("No fault found. Good flight.".into());
        }

        let timeline = f.events.iter().map(|e| format!("T+{:>5.0}s  {:>6.1} km  {}", e.t, e.alt / 1e3, e.msg)).collect();
        Self {
            outcome, headline, diagnosis: d, timeline,
            max_alt: f.max_alt, max_speed: f.max_speed, max_q: f.max_q, dv_used: f.dv_used,
            dv_design, duration: f.t, orbit,
        }
    }
}

impl FlightReport {
    /// Reports for flights away from the Earth pad: descents from orbit, landings on other
    /// bodies and ascents from them.
    fn lander_report(f: &Flight) -> Self {
        let b = f.body;
        let dv_design = f.vessel.total_dv();
        let mut d: Vec<String> = vec![];
        let orbit = f.reached_orbit.then(|| (f.peri_alt(), f.apo_alt()));
        let (outcome, headline) = match f.status {
            Status::Landed { speed } if f.from_orbit => (Outcome::Landed, format!("Landed on {} at {:.1} m/s", b.name, speed)),
            Status::Landed { speed } => (Outcome::Landed, format!("Touched down on {} at {:.1} m/s", b.name, speed)),
            Status::Crashed { speed } => (Outcome::Crashed, format!("Crashed into {} at {:.0} m/s", b.name, speed)),
            Status::BrokeUp { part, .. } => (Outcome::BrokeUp, format!("Vessel broke apart: {part} failed at max-Q")),
            _ if f.reached_orbit && !f.from_orbit => (Outcome::Orbit, format!("Orbit achieved over {}: {:.0} x {:.0} km", b.name, f.peri_alt() / 1e3, f.apo_alt() / 1e3)),
            _ if f.status == Status::OnPad || f.liftoff_twr < 0.0 => (Outcome::NeverLaunched, format!("Never left the surface of {}", b.name)),
            _ => (Outcome::Aborted, "Flight ended".into()),
        };
        match f.status {
            Status::Crashed { speed } => {
                let g = b.surface_gravity();
                d.push(format!("Impact speed was {speed:.0} m/s; a safe touchdown is under {:.0} m/s.", crate::sim::SAFE_LANDING));
                if let Some(e) = f.events.iter().find(|e| e.msg.contains("out of propellant")) {
                    d.push(format!("The last stage ran dry at {:.1} km altitude, {:.0} s into the descent. Landing from a {:.0} km orbit needs about {:.0} m/s of delta-v.",
                        e.alt / 1e3, e.t, b.park_alt / 1e3, crate::bodies::landing_dv(b, b.park_radius())));
                } else if f.throttle == 0.0 || f.dv_used < 50.0 {
                    d.push("The engine was never lit to slow the descent. Burn retrograde to lose orbital speed first.".into());
                } else {
                    let a = f.vessel.stage_engines(f.stage).0 * 1000.0 / f.mass();
                    d.push(format!("Braking started too late for the engine: thrust gives {:.1} m/s^2 against {:.2} m/s^2 of gravity. Light the engine higher, or use a higher thrust-to-weight lander.", a, g));
                }
                if b.has_atmosphere() && !f.chute_deployed {
                    d.push(format!("{} has an atmosphere; a parachute helps shed speed before the final burn.", b.name));
                }
            }
            Status::Landed { .. } => d.push(format!("Used {:.0} m/s of {:.0} m/s design delta-v.", f.dv_used, dv_design)),
            _ if outcome == Outcome::NeverLaunched => d.push(format!("Liftoff needs thrust-to-weight above 1 against {:.2} m/s^2 of gravity here.", b.surface_gravity())),
            _ if outcome == Outcome::Orbit => d.push(format!("Used {:.0} m/s of {:.0} m/s design delta-v.", f.dv_used, dv_design)),
            _ => {}
        }
        if d.is_empty() {
            d.push("No fault found.".into());
        }
        let timeline = f.events.iter().map(|e| format!("T+{:>5.0}s  {:>6.1} km  {}", e.t, e.alt / 1e3, e.msg)).collect();
        Self {
            outcome, headline, diagnosis: d, timeline,
            max_alt: f.max_alt, max_speed: f.max_speed, max_q: f.max_q, dv_used: f.dv_used,
            dv_design, duration: f.t, orbit,
        }
    }
}

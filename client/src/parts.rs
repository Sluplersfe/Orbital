//! Part catalog. The same definitions drive the builder UI and the physics.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Engine { thrust_kn: f64, isp_sl: f64, isp_vac: f64 },
    Tank { prop_kg: f64 },
    /// Command pod / payload. Always needed on top for a recoverable flight.
    Pod,
    Payload,
    Parachute,
    NoseCone,
    Structure,
}

#[derive(Debug, Clone, Copy)]
pub struct Part {
    pub id: &'static str,
    pub name: &'static str,
    /// Era the part belongs to (1 or 2).
    pub era: u8,
    /// Research points to unlock; 0 = available from the start.
    pub cost_rp: u32,
    /// Dry mass (kg).
    pub mass: f64,
    /// Max dynamic pressure the part survives (Pa).
    pub strength_pa: f64,
    pub kind: Kind,
}

const fn p(id: &'static str, name: &'static str, era: u8, cost_rp: u32, mass: f64, strength_kpa: f64, kind: Kind) -> Part {
    Part { id, name, era, cost_rp, mass, strength_pa: strength_kpa * 1000.0, kind }
}

pub const CATALOG: &[Part] = &[
    // Era 1: suborbital
    p("pod1", "Mk1 Pod", 1, 0, 800.0, 80.0, Kind::Pod),
    p("chute1", "Parachute", 1, 0, 100.0, 40.0, Kind::Parachute),
    p("spark", "Spark Engine", 1, 0, 1500.0, 80.0, Kind::Engine { thrust_kn: 250.0, isp_sl: 250.0, isp_vac: 285.0 }),
    p("tank_s", "Small Tank", 1, 0, 250.0, 80.0, Kind::Tank { prop_kg: 2250.0 }),
    p("tank_m", "Medium Tank", 1, 0, 800.0, 70.0, Kind::Tank { prop_kg: 7200.0 }),
    p("cone1", "Nose Cone", 1, 5, 100.0, 80.0, Kind::NoseCone),
    p("strut1", "Reinforced Ring", 1, 10, 150.0, 120.0, Kind::Structure),
    // Era 2: orbit
    p("vector", "Vector Engine", 2, 40, 3000.0, 80.0, Kind::Engine { thrust_kn: 700.0, isp_sl: 275.0, isp_vac: 315.0 }),
    p("vacuum", "Vacuum Engine", 2, 40, 700.0, 80.0, Kind::Engine { thrust_kn: 90.0, isp_sl: 140.0, isp_vac: 360.0 }),
    p("tank_l", "Large Tank", 2, 30, 2400.0, 60.0, Kind::Tank { prop_kg: 21600.0 }),
    p("tank_xl", "Huge Tank", 2, 60, 6500.0, 55.0, Kind::Tank { prop_kg: 58500.0 }),
    p("sat1", "Satellite", 2, 25, 500.0, 80.0, Kind::Payload),
    p("crew1", "Crew Capsule", 2, 50, 1800.0, 80.0, Kind::Pod),
];

pub fn get(id: &str) -> Option<&'static Part> {
    CATALOG.iter().find(|p| p.id == id)
}

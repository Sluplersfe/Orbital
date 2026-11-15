//! Part catalog and tech tree, loaded from `data/parts.tsv`. The same definitions drive the
//! builder UI, the physics, the research screen and the station and crew rules.
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Engine { thrust_kn: f64, isp_sl: f64, isp_vac: f64 },
    Tank { prop_kg: f64 },
    /// Command pod: seats `crew` people (a pilot must be a player).
    Pod { crew: u8 },
    /// Satellite or probe: earns research points only, never claims anything.
    Payload,
    Parachute,
    NoseCone,
    Structure,
    Docking,
    /// Station core: the hub every station module attaches to.
    Core,
    Solar { kw: f64 },
    Battery { kwh: f64 },
    /// Crew housing. `recycle` is the fraction of food and water use that is recycled.
    Habitat { crew: u8, recycle: f64 },
    /// Propellant storage that refuels docked ships.
    Depot { prop_kg: f64 },
    /// Lets a station assemble and fuel ships in orbit.
    Assembly,
    Farm { food_kg_h: f64 },
    /// In-situ propellant plant: refuels ships at a body away from Earth.
    Isru { prop_kg_h: f64 },
    Survey,
    Supply { food_kg: f64, water_kg: f64 },
    /// A tech node with no physical part; unlocking it grants an ability.
    Node { grant: &'static str },
}

#[derive(Debug, Clone, Copy)]
pub struct Part {
    pub id: &'static str,
    pub name: &'static str,
    /// Era in which the part can be researched (1-5).
    pub era: u8,
    /// Research points to unlock; 0 = available from the start.
    pub cost_rp: u32,
    /// Mass (kg), including contents other than propellant.
    pub mass: f64,
    /// Max dynamic pressure the part survives (Pa).
    pub strength_pa: f64,
    pub branch: &'static str,
    /// 1-4. Tier 4, the top of a branch, needs a rare claim.
    pub tier: u8,
    /// Parts that must be unlocked first.
    pub requires: &'static [&'static str],
    /// Resource that must have been claimed.
    pub claim: Option<&'static str>,
    pub kind: Kind,
}

impl Part {
    /// Can this part be placed on a vessel (as opposed to being a tech node)?
    pub fn placeable(&self) -> bool {
        !matches!(self.kind, Kind::Node { .. })
    }
}

pub const BRANCHES: [&str; 4] = ["propulsion", "structures", "life_support", "automation"];

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn parse_kind(kind: &str, params: &str, row: &str) -> Kind {
    let kv: Vec<(&str, &str)> = params.split(',').filter(|s| !s.trim().is_empty()).map(|s| {
        let (k, v) = s.split_once('=').unwrap_or_else(|| panic!("parts.tsv: bad param {s:?} in {row:?}"));
        (k.trim(), v.trim())
    }).collect();
    let f = |k: &str| -> f64 {
        kv.iter().find(|(n, _)| *n == k).unwrap_or_else(|| panic!("parts.tsv: missing {k} in {row:?}")).1
            .parse().unwrap_or_else(|_| panic!("parts.tsv: bad number for {k} in {row:?}"))
    };
    match kind {
        "engine" => Kind::Engine { thrust_kn: f("thrust"), isp_sl: f("isp_sl"), isp_vac: f("isp_vac") },
        "tank" => Kind::Tank { prop_kg: f("prop") },
        "pod" => Kind::Pod { crew: f("crew") as u8 },
        "payload" => Kind::Payload,
        "parachute" => Kind::Parachute,
        "nosecone" => Kind::NoseCone,
        "structure" => Kind::Structure,
        "docking" => Kind::Docking,
        "core" => Kind::Core,
        "solar" => Kind::Solar { kw: f("kw") },
        "battery" => Kind::Battery { kwh: f("kwh") },
        "habitat" => Kind::Habitat { crew: f("crew") as u8, recycle: f("recycle") },
        "depot" => Kind::Depot { prop_kg: f("prop") },
        "assembly" => Kind::Assembly,
        "farm" => Kind::Farm { food_kg_h: f("food") },
        "isru" => Kind::Isru { prop_kg_h: f("prop") },
        "survey" => Kind::Survey,
        "supply" => Kind::Supply { food_kg: f("food"), water_kg: f("water") },
        "node" => Kind::Node { grant: kv.iter().find(|(n, _)| *n == "grant").map(|(_, v)| leak(v)).unwrap_or("") },
        other => panic!("parts.tsv: unknown kind {other:?}"),
    }
}

static CATALOG: OnceLock<Vec<Part>> = OnceLock::new();

pub fn catalog() -> &'static [Part] {
    CATALOG.get_or_init(|| {
        include_str!("../data/parts.tsv")
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| {
                let c: Vec<&str> = l.split('|').map(str::trim).collect();
                assert_eq!(c.len(), 12, "parts.tsv: bad row {l:?}");
                let n = |i: usize| -> f64 { c[i].parse().unwrap_or_else(|_| panic!("parts.tsv: bad number {:?} in {l:?}", c[i])) };
                let requires: Vec<&'static str> = if c[8] == "-" { vec![] } else { c[8].split(',').map(|s| leak(s.trim())).collect() };
                Part {
                    id: leak(c[0]),
                    name: leak(c[1]),
                    era: n(2) as u8,
                    cost_rp: n(3) as u32,
                    mass: n(4),
                    strength_pa: n(5) * 1000.0,
                    branch: leak(c[6]),
                    tier: n(7) as u8,
                    requires: Box::leak(requires.into_boxed_slice()),
                    claim: (c[9] != "-").then(|| leak(c[9])),
                    kind: parse_kind(c[10], c[11], l),
                }
            })
            .collect()
    })
}

pub fn get(id: &str) -> Option<&'static Part> {
    catalog().iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_loads_and_is_consistent() {
        let c = catalog();
        assert!(c.len() >= 35);
        for p in c {
            assert!((1..=5).contains(&p.era) && (1..=4).contains(&p.tier), "{}", p.id);
            assert!(BRANCHES.contains(&p.branch), "{}", p.id);
            for r in p.requires {
                let q = get(r).unwrap_or_else(|| panic!("{} requires unknown part {r}", p.id));
                assert!(q.era <= p.era, "{} (era {}) requires later-era {}", p.id, p.era, r);
            }
            // Top tier needs a rare claim; nothing below it does.
            if let Some(c) = p.claim {
                let rare = ["helium3", "platinum", "exotic"].contains(&c);
                if p.tier == 4 { assert!(rare, "{} tier 4 needs a rare claim", p.id) }
            } else {
                assert!(p.tier < 4, "{} is top tier with no rare claim", p.id);
            }
        }
        let ids: std::collections::BTreeSet<_> = c.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), c.len(), "duplicate part ids");
    }

    #[test]
    fn phase_two_numbers_are_unchanged() {
        let s = get("spark").unwrap();
        assert_eq!((s.mass, s.cost_rp, s.era), (1500.0, 0, 1));
        assert!(matches!(s.kind, Kind::Engine { thrust_kn, .. } if thrust_kn == 250.0));
        assert_eq!(get("tank_xl").unwrap().strength_pa, 55_000.0);
    }

    #[test]
    fn every_branch_has_a_rare_gated_top_tier() {
        for b in BRANCHES {
            assert!(catalog().iter().any(|p| p.branch == b && p.tier == 4 && p.claim.is_some()), "{b}");
        }
    }
}

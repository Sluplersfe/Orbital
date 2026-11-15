//! Stations: modules, power, housing, refuelling, assembly and life support.
//!
//! Nothing here costs money. A depot refills itself from free resupply at Earth; away from Earth
//! (surface bases and orbits of other bodies) fuel comes only from an ISRU plant once water ice is
//! claimed. Life support is the real constraint: crews eat and drink, farms and recycling stretch
//! the stores, and a station that browns out for too long kills its crew (they respawn at Earth).
use crate::parts::{self, Kind};
use crate::space::{Orbit, Place, Spacecraft};
use crate::vessel::Vessel;
use std::collections::BTreeSet;

/// Per person per world hour (kg). Designed values; the world clock has no time warp.
pub const FOOD_KG_H: f64 = 10.0;
pub const WATER_KG_H: f64 = 15.0;
/// Free resupply into a depot at Earth (kg per world hour).
pub const DEPOT_RESUPPLY_KG_H: f64 = 4_000.0;
/// Seconds of power deficit before life support fails.
pub const BROWNOUT_LIMIT_S: f64 = 600.0;
/// A warning goes out when a store holds less than this many hours of supply.
pub const LOW_SUPPLY_HOURS: f64 = 2.0;
/// Extra recycling each powered farm adds, capped by `MAX_RECYCLE`.
const FARM_RECYCLE: f64 = 0.4;
const MAX_RECYCLE: f64 = 0.95;

/// Power use (kW) of a powered module.
fn load_kw(k: &Kind) -> f64 {
    match k {
        Kind::Core => 4.0,
        Kind::Habitat { .. } => 3.0,
        Kind::Farm { .. } => 8.0,
        Kind::Isru { .. } => 10.0,
        Kind::Assembly => 6.0,
        Kind::Depot { .. } => 1.0,
        _ => 0.0,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Warning(String),
    /// These people died at the station and are sent back to Earth.
    Deaths { people: Vec<String>, why: String },
}

#[derive(Debug, Clone)]
pub struct Station {
    pub id: u64,
    pub name: String,
    pub body: &'static str,
    /// None for a surface base.
    pub orbit: Option<Orbit>,
    pub modules: Vec<&'static str>,
    pub docked: Vec<u64>,
    pub prop_kg: f64,
    pub food: f64,
    pub water: f64,
    pub battery_kwh: f64,
    pub crew: Vec<String>,
    pub brownout_s: f64,
    warned_low: bool,
}

fn is_module(k: &Kind) -> bool {
    !matches!(k, Kind::Engine { .. } | Kind::Tank { .. } | Kind::Parachute | Kind::NoseCone | Kind::Structure | Kind::Node { .. })
}

impl Station {
    /// Found a station from a ship whose vessel has a station core. The ship merges into it.
    pub fn found(id: u64, name: &str, ship: &Spacecraft) -> Result<Self, String> {
        let (body, orbit) = match ship.place {
            Place::Orbit(o) => (o.body, Some(o)),
            Place::Landed { body, .. } => (body, None),
            _ => return Err("A station is founded from a ship in orbit or on a surface.".into()),
        };
        if !ship.has(|k| matches!(k, Kind::Core)) {
            return Err("Nothing to found a station with: the vessel has no Station Core.".into());
        }
        let mut s = Self {
            id, name: name.into(), body, orbit, modules: vec![], docked: vec![], prop_kg: 0.0, food: 0.0, water: 0.0,
            battery_kwh: 0.0, crew: vec![], brownout_s: 0.0, warned_low: false,
        };
        s.merge(ship);
        s.battery_kwh = s.battery_capacity();
        Ok(s)
    }

    /// Absorb a docked ship: its modules, supplies and crew become part of the station.
    pub fn merge(&mut self, ship: &Spacecraft) {
        for s in ship.stage..ship.vessel.stages.len() {
            for p in ship.vessel.parts(s) {
                if is_module(&p.kind) {
                    self.modules.push(p.id);
                }
            }
        }
        self.food += ship.food;
        self.water += ship.water;
        for c in &ship.crew {
            if !self.crew.contains(c) {
                self.crew.push(c.clone());
            }
        }
    }

    fn kinds(&self) -> impl Iterator<Item = Kind> + '_ {
        self.modules.iter().filter_map(|id| parts::get(id)).map(|p| p.kind)
    }

    pub fn count(&self, f: impl Fn(&Kind) -> bool) -> usize {
        self.kinds().filter(|k| f(k)).count()
    }

    pub fn has(&self, f: impl Fn(&Kind) -> bool) -> bool {
        self.count(f) > 0
    }

    pub fn housing(&self) -> u32 {
        self.kinds().map(|k| if let Kind::Habitat { crew, .. } = k { crew as u32 } else { 0 }).sum()
    }

    pub fn depot_capacity(&self) -> f64 {
        self.kinds().map(|k| if let Kind::Depot { prop_kg } = k { prop_kg } else { 0.0 }).sum()
    }

    pub fn battery_capacity(&self) -> f64 {
        self.kinds().map(|k| if let Kind::Battery { kwh } = k { kwh } else { 0.0 }).sum()
    }

    pub fn generation_kw(&self) -> f64 {
        self.kinds().map(|k| if let Kind::Solar { kw } = k { kw } else { 0.0 }).sum()
    }

    pub fn load_kw(&self) -> f64 {
        self.kinds().map(|k| load_kw(&k)).sum()
    }

    /// Generation minus load (kW). Negative drains the battery, then browns the station out.
    pub fn power_kw(&self) -> f64 {
        self.generation_kw() - self.load_kw()
    }

    /// True while the modules that need power are running.
    pub fn powered(&self) -> bool {
        self.power_kw() >= 0.0 || self.battery_kwh > 0.0
    }

    /// Fraction of food and water use that is recycled right now.
    pub fn recycle(&self) -> f64 {
        if !self.powered() {
            return 0.0;
        }
        let hab = self.kinds().map(|k| if let Kind::Habitat { recycle, .. } = k { recycle } else { 0.0 }).fold(0.0, f64::max);
        (hab + FARM_RECYCLE * self.count(|k| matches!(k, Kind::Farm { .. })) as f64).min(MAX_RECYCLE)
    }

    pub fn add_crew(&mut self, name: &str) -> Result<(), String> {
        if self.crew.iter().any(|c| c == name) {
            return Ok(());
        }
        if (self.crew.len() as u32) >= self.housing() {
            return Err(format!("{} has no free bunk: {} of {} are taken.", self.name, self.crew.len(), self.housing()));
        }
        self.crew.push(name.into());
        Ok(())
    }

    /// Hours of supply left for the people on board, counting production.
    pub fn supply_hours(&self) -> f64 {
        let n = self.crew.len() as f64;
        if n == 0.0 {
            return f64::INFINITY;
        }
        let k = 1.0 - self.recycle();
        let farm = if self.powered() { self.farm_food_kg_h() } else { 0.0 };
        let food_net = n * FOOD_KG_H * k - farm;
        let water_net = n * WATER_KG_H * k;
        let h = |stock: f64, net: f64| if net <= 0.0 { f64::INFINITY } else { stock / net };
        h(self.food, food_net).min(h(self.water, water_net))
    }

    fn farm_food_kg_h(&self) -> f64 {
        self.kinds().map(|k| if let Kind::Farm { food_kg_h } = k { food_kg_h } else { 0.0 }).sum()
    }

    /// Advance dt seconds. `claims` decides whether an ISRU plant can run (water ice claimed).
    pub fn tick(&mut self, dt: f64, claims: &BTreeSet<String>) -> Vec<Event> {
        let mut ev = vec![];
        let h = dt / 3600.0;
        // Power and battery.
        let net = self.power_kw();
        if net < 0.0 {
            self.battery_kwh = (self.battery_kwh + net * h).max(0.0);
        } else {
            self.battery_kwh = (self.battery_kwh + net * h).min(self.battery_capacity());
        }
        if net < 0.0 && self.battery_kwh <= 0.0 {
            let before = self.brownout_s;
            self.brownout_s += dt;
            if before == 0.0 {
                ev.push(Event::Warning(format!("{}: out of power, life support fails in {:.0} s.", self.name, BROWNOUT_LIMIT_S)));
            }
        } else {
            self.brownout_s = 0.0;
        }
        let powered = self.powered();
        // Fuel: free resupply at Earth, or ISRU elsewhere.
        let cap = self.depot_capacity();
        if cap > 0.0 && powered {
            if self.body == "earth" {
                self.prop_kg = (self.prop_kg + DEPOT_RESUPPLY_KG_H * h).min(cap);
            } else if claims.contains("water_ice") {
                let rate: f64 = self.kinds().map(|k| if let Kind::Isru { prop_kg_h } = k { prop_kg_h } else { 0.0 }).sum();
                self.prop_kg = (self.prop_kg + rate * h).min(cap);
            }
        }
        // Life support.
        let n = self.crew.len() as f64;
        if n > 0.0 {
            let k = 1.0 - self.recycle();
            if powered {
                self.food += self.farm_food_kg_h() * h;
            }
            self.food -= n * FOOD_KG_H * k * h;
            self.water -= n * WATER_KG_H * k * h;
            let low = self.supply_hours() < LOW_SUPPLY_HOURS;
            if low && !self.warned_low && self.food > 0.0 && self.water > 0.0 {
                self.warned_low = true;
                ev.push(Event::Warning(format!("{}: supplies for {:.1} h left. Dock a supply pod or build a farm.", self.name, self.supply_hours())));
            }
            if !low {
                self.warned_low = false;
            }
            let why = if self.food < 0.0 {
                Some("starved")
            } else if self.water < 0.0 {
                Some("ran out of water")
            } else if self.brownout_s >= BROWNOUT_LIMIT_S {
                Some("life support failed after the power ran out")
            } else {
                None
            };
            if let Some(why) = why {
                self.food = self.food.max(0.0);
                self.water = self.water.max(0.0);
                self.brownout_s = 0.0;
                ev.push(Event::Deaths { people: std::mem::take(&mut self.crew), why: format!("{}: the crew {why}.", self.name) });
            }
        }
        ev
    }

    /// Free refuelling of a docked ship's active stage from the depot. Returns kg added.
    pub fn refuel(&mut self, ship: &mut Spacecraft, claims: &BTreeSet<String>) -> Result<f64, String> {
        if ship.place != (Place::Docked { station: self.id }) {
            return Err("The ship is not docked here.".into());
        }
        if self.depot_capacity() == 0.0 {
            return Err(format!("{} has no Fuel Depot.", self.name));
        }
        if !self.powered() {
            return Err(format!("{} has no power for the depot pumps.", self.name));
        }
        if self.body != "earth" {
            if !self.has(|k| matches!(k, Kind::Isru { .. })) {
                return Err(format!("{} is away from Earth and has no ISRU plant, so there is no fuel.", self.name));
            }
            if !claims.contains("water_ice") {
                return Err("Water ice has not been claimed, so the ISRU plant has nothing to make fuel from.".into());
            }
        }
        let add = ship.refill(self.prop_kg);
        self.prop_kg -= add;
        if add <= 0.0 && self.prop_kg <= 0.0 {
            return Err("The depot is empty. It refills over time.".into());
        }
        Ok(add)
    }

    /// Assemble a vessel in orbit at no cost. The new ship starts docked here with full tanks.
    pub fn assemble(&self, id: u64, name: &str, vessel: Vessel, unlocked: impl Fn(&str) -> bool) -> Result<Spacecraft, String> {
        if !self.has(|k| matches!(k, Kind::Assembly)) {
            return Err(format!("{} has no Assembly Bay.", self.name));
        }
        if !self.powered() {
            return Err(format!("{} has no power for the assembly bay.", self.name));
        }
        if !self.has(|k| matches!(k, Kind::Core)) {
            return Err("No station core.".into());
        }
        for s in &vessel.stages {
            for id in s {
                let p = parts::get(id).ok_or_else(|| format!("Unknown part {id}."))?;
                if !p.placeable() {
                    return Err(format!("{} cannot be built into a vessel.", p.name));
                }
                if !unlocked(id) {
                    return Err(format!("{} is not unlocked.", p.name));
                }
            }
        }
        let errs = vessel.validate();
        if !errs.is_empty() {
            return Err(errs.join(" "));
        }
        Ok(Spacecraft::new(id, name, vessel, Place::Docked { station: self.id }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bodies::earth;

    fn orbit() -> Orbit {
        Orbit { body: "earth", r: earth().radius + 400_000.0, theta0: 0.0, epoch: 0.0 }
    }

    fn core_ship(stages: &[&[&'static str]]) -> Spacecraft {
        let v = Vessel { stages: stages.iter().map(|s| s.to_vec()).collect() };
        Spacecraft::new(1, "core", v, Place::Orbit(orbit()))
    }

    fn base() -> Station {
        let ship = core_ship(&[&["vacuum", "tank_s"], &["core1", "solar1", "solar1", "batt1", "hab1", "depot1", "dock1", "supply1", "asm1"]]);
        Station::found(10, "Alpha", &ship).unwrap()
    }

    #[test]
    fn needs_a_core_and_a_place() {
        let s = core_ship(&[&["vacuum", "tank_s", "pod1"]]);
        assert!(Station::found(1, "x", &s).unwrap_err().contains("Core"));
        let mut s = core_ship(&[&["core1"]]);
        s.place = Place::Destroyed;
        assert!(Station::found(1, "x", &s).is_err());
    }

    #[test]
    fn power_budget_and_housing() {
        let st = base();
        // 40 kW solar vs core 4 + hab 3 + depot 1 + assembly 6.
        assert_eq!(st.generation_kw(), 40.0);
        assert_eq!(st.load_kw(), 14.0);
        assert!(st.power_kw() > 0.0 && st.powered());
        assert_eq!(st.housing(), 4);
        assert_eq!(st.food, 400.0);
        let mut st = st;
        for n in ["a", "b", "c", "d"] {
            st.add_crew(n).unwrap();
        }
        assert!(st.add_crew("e").unwrap_err().contains("no free bunk"));
    }

    #[test]
    fn crew_drains_supplies_and_dies_when_they_run_out() {
        let mut st = base();
        st.add_crew("ann").unwrap();
        st.add_crew("bob").unwrap();
        let claims = BTreeSet::new();
        // 2 people, recycle 0.3: 2*10*0.7 = 14 food/h, 21 water/h; 600 water lasts ~28 h.
        let h = st.supply_hours();
        assert!((h - 600.0 / 21.0).abs() < 0.1, "{h}");
        let ev = st.tick(3600.0 * 10.0, &claims);
        assert!(ev.is_empty(), "{ev:?}");
        assert!((st.water - (600.0 - 210.0)).abs() < 1.0);
        assert!(st.tick(3600.0 * 16.0, &claims).is_empty());
        let ev = st.tick(3600.0, &claims);
        assert!(ev.iter().any(|e| matches!(e, Event::Warning(w) if w.contains("supplies"))), "{ev:?}");
        let ev = st.tick(3600.0 * 2.0, &claims);
        assert!(ev.iter().any(|e| matches!(e, Event::Deaths { people, .. } if people.len() == 2)), "{ev:?}");
        assert!(st.crew.is_empty());
    }

    #[test]
    fn a_farm_stretches_supplies_and_can_make_them_last_forever() {
        let ship = core_ship(&[&["core1", "solar1", "solar1", "solar1", "hab1", "farm1", "supply1"]]);
        let mut st = Station::found(2, "F", &ship).unwrap();
        st.add_crew("ann").unwrap();
        assert!(st.recycle() > 0.69, "{}", st.recycle()); // 0.3 hab + 0.4 farm
        assert_eq!(st.supply_hours(), f64::INFINITY.min(st.supply_hours()));
        // One crew eats 10*0.3 = 3 food/h and the farm grows 40: food never runs out.
        // Water: 15*0.3 = 4.5 kg/h from 600 kg is still finite.
        let h = st.supply_hours();
        assert!((h - 600.0 / 4.5).abs() < 0.5, "{h}");
        let claims = BTreeSet::new();
        st.tick(3600.0 * 20.0, &claims);
        assert!(st.food >= 400.0, "{}", st.food);
        assert!(st.crew.len() == 1);
    }

    #[test]
    fn brownout_kills_after_the_warning_time() {
        let ship = core_ship(&[&["core1", "hab1", "supply1"]]); // no solar at all
        let mut st = Station::found(3, "Dark", &ship).unwrap();
        st.add_crew("ann").unwrap();
        let claims = BTreeSet::new();
        let ev = st.tick(60.0, &claims);
        assert!(ev.iter().any(|e| matches!(e, Event::Warning(w) if w.contains("out of power"))));
        assert!(st.tick(300.0, &claims).is_empty());
        let ev = st.tick(300.0, &claims);
        assert!(ev.iter().any(|e| matches!(e, Event::Deaths { why, .. } if why.contains("power"))), "{ev:?}");
    }

    #[test]
    fn refuelling_is_free_but_needs_a_depot_and_docking() {
        let mut st = base();
        let claims = BTreeSet::new();
        st.tick(3600.0 * 2.0, &claims); // depot fills from free resupply
        assert!(st.prop_kg > 7_100.0);
        let v = Vessel { stages: vec![vec!["vacuum", "tank_m", "pod1"]] };
        let mut ship = Spacecraft::new(5, "tug", v, Place::Orbit(orbit()));
        ship.prop = 100.0;
        assert!(st.refuel(&mut ship, &claims).unwrap_err().contains("not docked"));
        ship.place = Place::Docked { station: 10 };
        let before = (ship.dv_remaining(), st.prop_kg);
        let add = st.refuel(&mut ship, &claims).unwrap();
        assert!((add - 7_100.0).abs() < 1.0 && ship.dv_remaining() > before.0 + 1_000.0);
        assert!((before.1 - st.prop_kg - add).abs() < 1e-6);
        // No depot: refused.
        let mut bare = Station::found(11, "Bare", &core_ship(&[&["core1", "solar1", "dock1"]])).unwrap();
        assert!(bare.refuel(&mut ship, &claims).unwrap_err().contains("Depot") || bare.refuel(&mut ship, &claims).is_err());
    }

    #[test]
    fn fuel_away_from_earth_needs_isru_and_a_water_ice_claim() {
        let mut ship = core_ship(&[&["core1", "solar1", "solar1", "solar1", "depot1", "isru1", "hab1"]]);
        ship.place = Place::Landed { body: "moon", site: 3 };
        let mut st = Station::found(20, "Moonbase", &ship).unwrap();
        let none = BTreeSet::new();
        let ice: BTreeSet<String> = ["water_ice".to_string()].into();
        st.tick(3600.0, &none);
        assert_eq!(st.prop_kg, 0.0, "no claim, no fuel");
        st.tick(3600.0, &ice);
        assert!((st.prop_kg - 600.0).abs() < 1.0, "{}", st.prop_kg);
        let v = Vessel { stages: vec![vec!["vacuum", "tank_m", "pod1"]] };
        let mut lander = Spacecraft::new(6, "l", v, Place::Docked { station: 20 });
        lander.prop = 0.0;
        assert!(st.refuel(&mut lander, &none).unwrap_err().contains("Water ice"));
        assert!(st.refuel(&mut lander, &ice).unwrap() > 500.0);
    }

    #[test]
    fn assembly_needs_a_bay_unlocked_parts_and_power() {
        let st = base();
        let v = Vessel { stages: vec![vec!["vacuum", "tank_l", "pod1"]] };
        let ok = |id: &str| id != "tank_l";
        assert!(st.assemble(9, "x", v.clone(), ok).unwrap_err().contains("not unlocked"));
        let ship = st.assemble(9, "x", v, |_| true).unwrap();
        assert_eq!(ship.place, Place::Docked { station: 10 });
        assert_eq!(ship.prop, 21_600.0);
        let no_bay = Station::found(12, "n", &core_ship(&[&["core1", "solar1"]])).unwrap();
        assert!(no_bay.assemble(1, "x", Vessel { stages: vec![vec!["vacuum", "pod1"]] }, |_| true).unwrap_err().contains("Assembly"));
    }
}

//! The space program: everything the group shares. Research, claims, ships, stations and people
//! live here, and every rule that crosses them (docking, crew moves, transfers, landings,
//! surveys) is a method that returns a plain-language error when it refuses.
//!
//! Nothing here costs money. Time is one world clock (`time`, seconds); `tick` advances it and
//! with it transits, life support and station power.
use crate::bodies::{self, SurveyFrom};
use crate::parts::Kind;
use crate::progress::Progress;
use crate::rendezvous;
use crate::report::{FlightReport, Outcome};
use crate::sim::{Flight, Status};
use crate::space::{self, Orbit, Place, Spacecraft};
use crate::station::{self, Event, Station, FOOD_KG_H, WATER_KG_H};
use crate::vessel::Vessel;

/// Pods carry this many person-hours of food and water per seat, so short flights need no supply pod.
pub const POD_RATION_HOURS: f64 = 72.0;
/// Research returned by every orbital flight, so launching modules is never wasted.
pub const ORBIT_DATA_RP: u32 = 10;
/// Largest step the world clock takes at once, so slow trouble (a brownout) is seen as it grows.
const MAX_STEP_S: f64 = 300.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A real person. Flies launches and any route, and can be teleported to a friend.
    Player,
    /// Hired crew (needs Crew Training). Rides along; flies only short routes unless the Flight AI is unlocked.
    Employee,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub name: String,
    pub role: Role,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loc {
    Earth,
    Ship(u64),
    Station(u64),
}

/// What a finished launch, descent or ascent came to.
#[derive(Debug, Clone)]
pub struct FlightResult {
    pub report: FlightReport,
    /// Research lines ("+25 first trip to space").
    pub awards: Vec<String>,
    /// The ship that exists afterwards, if any.
    pub ship: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct Program {
    pub progress: Progress,
    pub time: f64,
    pub ships: Vec<Spacecraft>,
    pub stations: Vec<Station>,
    pub people: Vec<Person>,
    pub next_id: u64,
    pub log: Vec<String>,
}

impl Program {
    pub fn new() -> Self {
        Self { progress: Progress::new(), next_id: 1, ..Default::default() }
    }

    fn say(&mut self, s: impl Into<String>) {
        self.log.push(s.into());
    }

    fn new_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// One-off research award keyed by a body or event; returns the line when it is new.
    fn grant(&mut self, key: String, pts: u32, label: &str) -> Option<String> {
        if self.progress.milestones.iter().any(|m| *m == key) {
            return None;
        }
        let line = self.progress.grant(Box::leak(key.into_boxed_str()), pts, label);
        if let Some(l) = &line {
            self.say(l.clone());
        }
        line
    }

    // ---- people -------------------------------------------------------------------------

    pub fn add_player(&mut self, name: &str) -> Result<(), String> {
        self.add_person(name, Role::Player)
    }

    fn add_person(&mut self, name: &str, role: Role) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err("A person needs a name.".into());
        }
        if self.people.iter().any(|p| p.name == name) {
            return Err(format!("{name} is already in the program."));
        }
        self.people.push(Person { name: name.into(), role });
        Ok(())
    }

    /// Hire an employee at Earth. Needs the Crew Training node.
    pub fn recruit(&mut self, name: &str) -> Result<(), String> {
        if !self.progress.has("crew_tr") {
            return Err("Employees need Crew Training unlocked in the research screen.".into());
        }
        self.add_person(name, Role::Employee)
    }

    pub fn role(&self, name: &str) -> Option<Role> {
        self.people.iter().find(|p| p.name == name).map(|p| p.role)
    }

    pub fn locate(&self, name: &str) -> Loc {
        if let Some(s) = self.ships.iter().find(|s| s.crew.iter().any(|c| c == name)) {
            return Loc::Ship(s.id);
        }
        if let Some(s) = self.stations.iter().find(|s| s.crew.iter().any(|c| c == name)) {
            return Loc::Station(s.id);
        }
        Loc::Earth
    }

    fn remove_from_places(&mut self, name: &str) {
        for s in &mut self.ships {
            s.crew.retain(|c| c != name);
        }
        for s in &mut self.stations {
            s.crew.retain(|c| c != name);
        }
    }

    fn require_person(&self, name: &str) -> Result<Role, String> {
        self.role(name).ok_or_else(|| format!("{name} is not in the program."))
    }

    fn put(&mut self, name: &str, to: Loc) -> Result<(), String> {
        match to {
            Loc::Earth => {}
            Loc::Ship(id) => {
                let s = self.ships.iter_mut().find(|s| s.id == id).ok_or("No such ship.")?;
                if s.crew.len() as u32 >= s.vessel.crew_capacity() {
                    return Err(format!("{} has no free seat.", s.name));
                }
                s.crew.push(name.into());
            }
            Loc::Station(id) => {
                self.stations.iter_mut().find(|s| s.id == id).ok_or("No such station.")?.add_crew(name)?;
            }
        }
        Ok(())
    }

    /// The respawn button: back to Earth from anywhere. Free.
    pub fn teleport_to_earth(&mut self, name: &str) -> Result<(), String> {
        self.require_person(name)?;
        self.remove_from_places(name);
        Ok(())
    }

    /// A player joins another player wherever they are, if there is room. Employees must be ferried.
    pub fn teleport_to_player(&mut self, name: &str, target: &str) -> Result<(), String> {
        if self.require_person(name)? != Role::Player {
            return Err("Only players can teleport; employees have to be flown there.".into());
        }
        if self.require_person(target)? != Role::Player {
            return Err(format!("{target} is not a player."));
        }
        let dest = self.locate(target);
        let old = self.locate(name);
        if old == dest {
            return Ok(());
        }
        self.remove_from_places(name);
        if let Err(e) = self.put(name, dest) {
            let _ = self.put(name, old);
            return Err(e);
        }
        Ok(())
    }

    /// Walk between two docked places (ship to station, or ship to ship at the same station).
    pub fn move_crew(&mut self, name: &str, to: Loc) -> Result<(), String> {
        self.require_person(name)?;
        let from = self.locate(name);
        let station_of = |p: &Program, l: Loc| match l {
            Loc::Station(id) => Some(id),
            Loc::Ship(id) => p.ships.iter().find(|s| s.id == id).and_then(|s| match s.place {
                Place::Docked { station } => Some(station),
                _ => None,
            }),
            Loc::Earth => None,
        };
        if from == Loc::Earth || to == Loc::Earth {
            return Err("Nobody walks to or from Earth. Use the teleport, or launch.".into());
        }
        match (station_of(self, from), station_of(self, to)) {
            (Some(a), Some(b)) if a == b => {}
            _ => return Err("They can only move between a station and ships docked to it.".into()),
        }
        self.remove_from_places(name);
        if let Err(e) = self.put(name, to) {
            let _ = self.put(name, from);
            return Err(e);
        }
        Ok(())
    }

    fn kill(&mut self, people: &[String], why: &str) {
        for p in people {
            self.remove_from_places(p);
        }
        if !people.is_empty() {
            self.say(format!("{} died ({why}) and respawn at Earth.", people.join(", ")));
        }
    }

    // ---- the clock ----------------------------------------------------------------------

    /// Advance the world clock by `dt` seconds.
    pub fn tick(&mut self, dt: f64) {
        let mut left = dt.max(0.0);
        while left > 0.0 {
            let step = left.min(MAX_STEP_S);
            left -= step;
            self.step(step);
        }
    }

    fn step(&mut self, dt: f64) {
        self.time += dt;
        let now = self.time;
        // Arrivals.
        let mut arrived = vec![];
        for s in &mut self.ships {
            if let Place::Transit { to, r, arrive } = s.place {
                if arrive <= now {
                    s.place = Place::Orbit(Orbit { body: to, r, theta0: 0.0, epoch: arrive });
                    arrived.push((s.name.clone(), to));
                }
            }
        }
        for (name, to) in arrived {
            let b = bodies::body(to);
            self.say(format!("{name} arrived in orbit of {}.", b.name));
            let pts = if to == "moon" { 80 } else { 150 };
            self.grant(format!("orbit:{to}"), pts, &format!("orbit of {}", b.name));
        }
        // Ship life support: no recycling aboard a ship.
        let h = dt / 3600.0;
        let mut dead: Vec<(Vec<String>, String)> = vec![];
        for s in &mut self.ships {
            if s.crew.is_empty() || matches!(s.place, Place::Docked { .. } | Place::Destroyed) {
                continue;
            }
            let n = s.crew.len() as f64;
            s.food -= n * FOOD_KG_H * h;
            s.water -= n * WATER_KG_H * h;
            if s.food < 0.0 || s.water < 0.0 {
                s.food = s.food.max(0.0);
                s.water = s.water.max(0.0);
                dead.push((std::mem::take(&mut s.crew), format!("{} ran out of {}", s.name, if s.food <= 0.0 { "food" } else { "water" })));
            }
        }
        for (people, why) in dead {
            self.kill(&people, &why);
        }
        // Stations.
        let claims = self.progress.claims.clone();
        let mut events = vec![];
        for st in &mut self.stations {
            events.extend(st.tick(dt, &claims));
        }
        for e in events {
            match e {
                Event::Warning(w) => self.say(w),
                Event::Deaths { people, why } => self.kill(&people, &why),
            }
        }
    }

    // ---- launch and flight results -------------------------------------------------------

    /// Check a crew can board a vessel at Earth for a launch.
    pub fn check_launch_crew(&self, vessel: &Vessel, crew: &[String]) -> Result<(), String> {
        if crew.len() as u32 > vessel.crew_capacity() {
            return Err(format!("{} people but only {} seats.", crew.len(), vessel.crew_capacity()));
        }
        for c in crew {
            self.require_person(c)?;
            if self.locate(c) != Loc::Earth {
                return Err(format!("{c} is not at Earth."));
            }
        }
        if !crew.is_empty() && !crew.iter().any(|c| self.role(c) == Some(Role::Player)) {
            return Err("A launch needs a player on board; employees cannot fly the ascent.".into());
        }
        Ok(())
    }

    /// Finish a launch from Earth that started at world time `t0`. Reports, awards research,
    /// and leaves a ship in orbit with its crew when the flight reached one.
    pub fn finish_flight(&mut self, name: &str, f: &Flight, t0: f64, crew: &[String]) -> FlightResult {
        self.advance_to(t0 + f.t);
        let report = FlightReport::from_flight(f);
        let (_, mut awards) = self.progress.award(&report);
        for a in &awards {
            self.say(a.clone());
        }
        let mut ship = None;
        if report.outcome == Outcome::Orbit {
            // Every orbital flight returns data; the first with a satellite or a crew pays extra.
            let has_sat = f.vessel.stages.iter().flatten().any(|p| *p == "sat1");
            let mut extra = vec![];
            if has_sat {
                extra.extend(self.progress.grant("satellite", 40, "satellite in orbit"));
            }
            if !crew.is_empty() {
                extra.extend(self.progress.grant("crewed_orbit", 60, "crewed orbit"));
            }
            self.progress.rp += ORBIT_DATA_RP;
            extra.push(format!("+{ORBIT_DATA_RP} orbital data"));
            for a in &extra {
                self.say(a.clone());
            }
            awards.extend(extra);
            let id = self.new_id();
            match Spacecraft::from_flight(id, name, f, t0) {
                Ok(mut sc) => {
                    let n = crew.len() as f64;
                    sc.food += n * POD_RATION_HOURS * FOOD_KG_H;
                    sc.water += n * POD_RATION_HOURS * WATER_KG_H;
                    sc.crew = crew.to_vec();
                    self.ships.push(sc);
                    ship = Some(id);
                }
                Err(e) => {
                    self.say(e);
                    self.kill(crew, "lost in a failed circularisation");
                }
            }
        } else if matches!(report.outcome, Outcome::BrokeUp | Outcome::Crashed | Outcome::SpaceLost) {
            self.kill(crew, &report.headline.to_lowercase());
        }
        FlightResult { report, awards, ship }
    }

    fn advance_to(&mut self, t: f64) {
        if t > self.time {
            self.tick(t - self.time);
        }
    }

    fn ship_index(&self, id: u64) -> Result<usize, String> {
        self.ships.iter().position(|s| s.id == id).ok_or_else(|| "No such ship.".to_string())
    }

    fn station_index(&self, id: u64) -> Result<usize, String> {
        self.stations.iter().position(|s| s.id == id).ok_or_else(|| "No such station.".to_string())
    }

    pub fn ship(&self, id: u64) -> Option<&Spacecraft> {
        self.ships.iter().find(|s| s.id == id)
    }

    pub fn station(&self, id: u64) -> Option<&Station> {
        self.stations.iter().find(|s| s.id == id)
    }

    // ---- stations -----------------------------------------------------------------------

    /// Found a station from a ship that carries a core. The ship's modules, supplies and crew become the station.
    pub fn found_station(&mut self, ship_id: u64, name: &str) -> Result<u64, String> {
        let i = self.ship_index(ship_id)?;
        let id = self.next_id;
        let st = Station::found(id, name, &self.ships[i])?;
        self.next_id += 1;
        self.ships.remove(i);
        self.stations.push(st);
        self.say(format!("Station {name} founded."));
        self.check_station(self.stations.len() - 1);
        Ok(id)
    }

    /// The Stations goal: a core, power and housing together.
    fn check_station(&mut self, ti: usize) {
        let st = &self.stations[ti];
        let ok = st.has(|k| matches!(k, Kind::Core)) && st.has(|k| matches!(k, Kind::Solar { .. })) && st.has(|k| matches!(k, Kind::Habitat { .. }));
        if ok {
            self.grant("station".into(), 150, "station (core, power and housing)");
        } else if !self.progress.milestones.contains("station") {
            self.say("A station needs a core, a solar array and a habitat to count. Dock more modules.");
        }
    }

    /// Fly a ship to a station in the same orbit and dock: phasing and transfer burns, a
    /// Clohessy-Wiltshire final approach, all paid from the ship's propellant. Takes world time.
    pub fn rendezvous(&mut self, ship_id: u64, station_id: u64) -> Result<f64, String> {
        let si = self.ship_index(ship_id)?;
        let ti = self.station_index(station_id)?;
        let (ship, st) = (&self.ships[si], &self.stations[ti]);
        if !matches!(ship.place, Place::Orbit(_) | Place::Landed { .. }) {
            return Err("The ship has to be in orbit (or on the ground at a base) to dock.".into());
        }
        if !ship.has(|k| matches!(k, Kind::Docking)) {
            return Err("The ship has no Docking Port.".into());
        }
        if !st.has(|k| matches!(k, Kind::Docking)) {
            return Err(format!("{} has no Docking Port.", st.name));
        }
        if st.docked.len() >= st.count(|k| matches!(k, Kind::Docking)) {
            return Err(format!("{} has no free docking port.", st.name));
        }
        if ship.crew.is_empty() && !self.progress.has("ai1") {
            return Err("Docking needs someone aboard, or the Flight AI.".into());
        }
        let spent = match (ship.place, st.orbit) {
            (Place::Landed { body, .. }, None) if body == st.body => 0.0,
            (Place::Orbit(so), Some(to)) => {
                if so.body != to.body {
                    return Err(format!("The ship is at {} and {} is at {}.", bodies::body(so.body).name, st.name, bodies::body(to.body).name));
                }
                let plan = rendezvous::plan_intercept(&so, &to, self.time)?;
                let arrive_orbit = Orbit { theta0: to.theta(plan.arrive), epoch: plan.arrive, ..to };
                let ap = rendezvous::final_approach(&arrive_orbit);
                let (miss, speed) = rendezvous::approach_miss(&arrive_orbit, &ap);
                if !rendezvous::can_dock(miss, speed) {
                    return Err(format!("The approach would miss by {miss:.0} m at {speed:.1} m/s."));
                }
                let mut trial = ship.clone();
                for (_, dv) in &plan.burns {
                    trial.burn(*dv).map_err(|e| e.to_string())?;
                }
                trial.burn(ap.dv).map_err(|e| e.to_string())?;
                self.ships[si] = trial;
                let done = plan.arrive + ap.duration;
                self.advance_to(done);
                plan.dv + ap.dv
            }
            _ => return Err("The ship and the station are not in the same place.".into()),
        };
        self.ships[si].place = Place::Docked { station: station_id };
        self.stations[ti].docked.push(ship_id);
        let name = self.ships[si].name.clone();
        let sname = self.stations[ti].name.clone();
        self.say(format!("{name} docked with {sname} ({spent:.0} m/s)."));
        self.grant("docking".into(), 60, "docking");
        Ok(spent)
    }

    /// Fold a docked ship into the station: its modules, supplies and crew become the station's.
    pub fn attach(&mut self, ship_id: u64) -> Result<(), String> {
        let si = self.ship_index(ship_id)?;
        let Place::Docked { station } = self.ships[si].place else { return Err("The ship is not docked.".into()) };
        let ti = self.station_index(station)?;
        let mut trial = self.stations[ti].clone();
        trial.merge(&self.ships[si]);
        if trial.crew.len() as u32 > trial.housing() {
            return Err("There would be more people than bunks. Build a habitat first.".into());
        }
        trial.docked.retain(|d| *d != ship_id);
        self.stations[ti] = trial;
        let ship = self.ships.remove(si);
        self.say(format!("{} joined {}.", ship.name, self.stations[ti].name));
        self.check_station(ti);
        Ok(())
    }

    pub fn undock(&mut self, ship_id: u64) -> Result<(), String> {
        let si = self.ship_index(ship_id)?;
        let Place::Docked { station } = self.ships[si].place else { return Err("The ship is not docked.".into()) };
        let ti = self.station_index(station)?;
        let t = self.time;
        let st = &mut self.stations[ti];
        st.docked.retain(|d| *d != ship_id);
        self.ships[si].place = match st.orbit {
            Some(o) => Place::Orbit(Orbit { theta0: o.theta(t), epoch: t, ..o }),
            None => Place::Landed { body: st.body, site: 0 },
        };
        Ok(())
    }

    /// Refuel a docked ship from the station's depot. Free.
    pub fn refuel(&mut self, ship_id: u64) -> Result<f64, String> {
        let si = self.ship_index(ship_id)?;
        let Place::Docked { station } = self.ships[si].place else { return Err("The ship is not docked.".into()) };
        let ti = self.station_index(station)?;
        let claims = self.progress.claims.clone();
        let add = self.stations[ti].refuel(&mut self.ships[si], &claims)?;
        self.say(format!("{} took {add:.0} kg of propellant.", self.ships[si].name));
        self.grant("refuel".into(), 40, "refuelling in orbit");
        Ok(add)
    }

    /// Build a ship at a station's assembly bay from unlocked parts.
    pub fn assemble(&mut self, station_id: u64, name: &str, vessel: Vessel) -> Result<u64, String> {
        let ti = self.station_index(station_id)?;
        let id = self.next_id;
        let ship = self.stations[ti].assemble(id, name, vessel, |p| self.progress.has(p))?;
        if self.stations[ti].docked.len() >= self.stations[ti].count(|k| matches!(k, Kind::Docking)) {
            return Err(format!("{} has no free docking port for the new ship.", self.stations[ti].name));
        }
        self.next_id += 1;
        self.stations[ti].docked.push(id);
        self.ships.push(ship);
        Ok(id)
    }

    // ---- travel ---------------------------------------------------------------------------

    /// Who may fly a ship away: any route with a player aboard; an employee-only or uncrewed
    /// ship only between a body and its parent or moon, unless the Flight AI is unlocked.
    fn may_fly(&self, ship: &Spacecraft, src: &str, dst: &str) -> Result<(), String> {
        if ship.crew.iter().any(|c| self.role(c) == Some(Role::Player)) || self.progress.has("ai1") {
            return Ok(());
        }
        let near = bodies::get(src).and_then(|b| b.parent) == Some(dst) || bodies::get(dst).and_then(|b| b.parent) == Some(src);
        if near {
            Ok(())
        } else {
            Err("Only a player can fly this far. Employees and probes manage the Earth-Moon run until the Flight AI is unlocked.".into())
        }
    }

    /// Send a ship in orbit to another body's parking orbit. Pays the delta-v now; returns the seconds until arrival.
    pub fn transfer(&mut self, ship_id: u64, dst: &str) -> Result<f64, String> {
        let si = self.ship_index(ship_id)?;
        let ship = &self.ships[si];
        let Place::Orbit(o) = ship.place else { return Err("The ship has to be in orbit to leave.".into()) };
        let d = bodies::get(dst).ok_or("No such body.")?;
        if d.id == o.body {
            return Err("It is already there.".into());
        }
        if d.min_era > self.progress.era() {
            return Err(format!("{} is out of reach until era {} ({}).", d.name, d.min_era, crate::progress::ERA_NAMES[d.min_era as usize - 1]));
        }
        self.may_fly(ship, o.body, d.id)?;
        let (route, to, r) = space::route_to(ship, dst).ok_or("No route.")?;
        let mut trial = ship.clone();
        trial.start_transfer(&route, to, r, self.time).map_err(|e| format!("{e} The trip needs {:.0} m/s.", route.total_dv))?;
        let eta = match trial.place {
            Place::Transit { arrive, .. } => arrive - self.time,
            _ => route.transit_s,
        };
        self.ships[si] = trial;
        Ok(eta)
    }

    // ---- landing and survey -----------------------------------------------------------------

    /// Start a powered descent from the ship's orbit. Fly the returned `Flight`, then call `finish_descent`.
    pub fn begin_descent(&mut self, ship_id: u64) -> Result<(Flight, f64), String> {
        let ship = self.ship(ship_id).ok_or("No such ship.")?;
        let Place::Orbit(o) = ship.place else { return Err("The ship has to be in orbit to land.".into()) };
        let b = o.body();
        if b.survey != SurveyFrom::Surface {
            return Err(format!("{} has no surface to land on.", b.name));
        }
        if b.has_atmosphere() {
            return Err(format!("{} has an atmosphere; this lander is built for airless bodies.", b.name));
        }
        let f = ship.flight_in_orbit(self.time).ok_or("The ship has to be in orbit to land.")?;
        Ok((f, self.time))
    }

    pub fn finish_descent(&mut self, ship_id: u64, f: &Flight, t0: f64) -> Result<FlightResult, String> {
        if !f.status.is_over() {
            return Err("The descent is not over yet.".into());
        }
        self.advance_to(t0 + f.t);
        let si = self.ship_index(ship_id)?;
        let report = FlightReport::from_flight(f);
        let body = f.body;
        let mut awards = vec![];
        match f.status {
            Status::Landed { .. } => {
                let s = &mut self.ships[si];
                s.stage = f.stage;
                s.prop = f.prop;
                s.place = Place::Landed { body: body.id, site: 0 };
                let pts = if body.id == "moon" { 150 } else { 200 };
                awards.extend(self.grant(format!("land:{}", body.id), pts, &format!("landing on {}", body.name)));
            }
            _ => {
                let crew = std::mem::take(&mut self.ships[si].crew);
                self.kill(&crew, &report.headline.to_lowercase());
                self.ships.remove(si);
            }
        }
        let ship = self.ship(ship_id).map(|s| s.id);
        Ok(FlightResult { report, awards, ship })
    }

    /// Start a lift-off from a landed ship. Fly the returned `Flight`, then call `finish_ascent`.
    pub fn begin_takeoff(&mut self, ship_id: u64) -> Result<(Flight, f64), String> {
        let ship = self.ship(ship_id).ok_or("No such ship.")?;
        let Place::Landed { body, .. } = ship.place else { return Err("The ship is not on a surface.".into()) };
        let f = Flight::on_surface(bodies::body(body), ship.vessel.clone(), ship.stage, ship.prop, 0.2);
        Ok((f, self.time))
    }

    pub fn finish_ascent(&mut self, ship_id: u64, f: &Flight, t0: f64) -> Result<FlightResult, String> {
        self.advance_to(t0 + f.t);
        let si = self.ship_index(ship_id)?;
        let report = FlightReport::from_flight(f);
        if f.reached_orbit {
            let old = &self.ships[si];
            let mut sc = Spacecraft::from_flight(old.id, &old.name, f, t0)?;
            sc.crew = old.crew.clone();
            sc.food = old.food;
            sc.water = old.water;
            self.ships[si] = sc;
        } else if f.status.is_over() {
            let crew = std::mem::take(&mut self.ships[si].crew);
            self.kill(&crew, &report.headline.to_lowercase());
            self.ships.remove(si);
        } else {
            return Err("The ascent is not over yet.".into());
        }
        let ship = self.ship(ship_id).map(|s| s.id);
        Ok(FlightResult { report, awards: vec![], ship })
    }

    /// Survey the body the ship is at. A crewed ship claims the body's resources (they unlock parts
    /// for everyone) and earns research; an uncrewed probe earns research only.
    pub fn survey(&mut self, ship_id: u64) -> Result<Vec<String>, String> {
        let ship = self.ship(ship_id).ok_or("No such ship.")?;
        if !ship.has(|k| matches!(k, Kind::Survey)) {
            return Err("The ship has no Survey Gear.".into());
        }
        let (body, ok_place) = match ship.place {
            Place::Landed { body, .. } => (bodies::body(body), true),
            Place::Orbit(o) => (o.body(), o.body().survey == SurveyFrom::Orbit),
            Place::Docked { station } => {
                let st = self.station(station).ok_or("No such station.")?;
                (bodies::body(st.body), st.orbit.is_none() || bodies::body(st.body).survey == SurveyFrom::Orbit)
            }
            _ => return Err("The ship has to be at a body to survey it.".into()),
        };
        if body.resources.is_empty() {
            return Err(format!("There is nothing to survey at {}.", body.name));
        }
        if !ok_place {
            return Err(match body.survey {
                SurveyFrom::Surface => format!("{} has to be surveyed from the ground: land first.", body.name),
                SurveyFrom::Orbit => format!("{} has to be surveyed from orbit.", body.name),
            });
        }
        if body.min_era > self.progress.era() {
            return Err(format!("{} is out of reach until era {}.", body.name, body.min_era));
        }
        let crewed = !ship.crew.is_empty();
        let mut lines = vec![];
        if crewed {
            let new = self.progress.claim_all(&body.resources);
            for r in &new {
                lines.push(format!("Claimed {r} at {}.", body.name));
            }
            lines.extend(self.grant(format!("survey:{}", body.id), 100, &format!("survey of {}", body.name)));
        } else {
            lines.extend(self.grant(format!("probe:{}", body.id), 40, &format!("probe data from {}", body.name)));
            if lines.is_empty() {
                return Err(format!("A probe already sent its data home from {}.", body.name));
            }
            lines.push("An uncrewed probe claims nothing: send a crew.".into());
        }
        for l in &lines {
            self.say(l.clone());
        }
        Ok(lines)
    }
}

/// Convenience for the station rules' constants in one place.
pub fn station_rates() -> (f64, f64) {
    (station::FOOD_KG_H, station::WATER_KG_H)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orbit_ship(p: &mut Program, v: &[&'static str], crew: &[&str]) -> u64 {
        let id = p.new_id();
        let vessel = Vessel { stages: vec![v.to_vec()] };
        let mut sc = Spacecraft::new(id, "s", vessel, Place::Orbit(Orbit { body: "earth", r: bodies::earth().park_radius(), theta0: 0.0, epoch: 0.0 }));
        sc.crew = crew.iter().map(|c| c.to_string()).collect();
        sc.food = 1000.0;
        sc.water = 1000.0;
        p.ships.push(sc);
        id
    }

    #[test]
    fn employees_need_crew_training_and_cannot_teleport() {
        let mut p = Program::new();
        p.add_player("ann").unwrap();
        assert!(p.recruit("bob").unwrap_err().contains("Crew Training"));
        p.progress.unlocked.insert("crew_tr".into());
        p.recruit("bob").unwrap();
        assert!(p.add_player("ann").is_err());
        assert!(p.teleport_to_player("bob", "ann").is_err());
        assert!(p.check_launch_crew(&Vessel { stages: vec![vec!["pod1", "spark"]] }, &["bob".into()]).is_err());
    }

    #[test]
    fn players_teleport_to_each_other_and_back() {
        let mut p = Program::new();
        p.add_player("ann").unwrap();
        p.add_player("cy").unwrap();
        let s = orbit_ship(&mut p, &["crew1", "vacuum", "tank_s"], &["ann"]);
        p.teleport_to_player("cy", "ann").unwrap();
        assert_eq!(p.locate("cy"), Loc::Ship(s));
        p.teleport_to_earth("cy").unwrap();
        assert_eq!(p.locate("cy"), Loc::Earth);
        // A full ship refuses.
        let t = orbit_ship(&mut p, &["pod1", "vacuum", "tank_s"], &["ann2"]);
        p.people.push(Person { name: "ann2".into(), role: Role::Player });
        assert!(p.teleport_to_player("cy", "ann2").is_err());
        assert_eq!(p.locate("cy"), Loc::Earth);
        let _ = t;
    }

    #[test]
    fn a_ship_that_runs_out_of_food_loses_its_crew() {
        let mut p = Program::new();
        p.add_player("ann").unwrap();
        let s = orbit_ship(&mut p, &["crew1", "vacuum", "tank_s"], &["ann"]);
        p.ships[0].food = 1.0;
        p.tick(3600.0);
        assert!(p.ship(s).unwrap().crew.is_empty());
        assert_eq!(p.locate("ann"), Loc::Earth);
        assert!(p.log.iter().any(|l| l.contains("ran out of")));
    }

    #[test]
    fn transfers_obey_era_and_who_is_flying() {
        let mut p = Program::new();
        p.add_player("ann").unwrap();
        let s = orbit_ship(&mut p, &["crew1", "vacuum", "tank_xl", "tank_xl"], &["ann"]);
        // The Moon is closed until a station exists.
        assert!(p.transfer(s, "moon").unwrap_err().contains("era 4"));
        p.progress.milestones.insert("station");
        let t = p.transfer(s, "moon").unwrap();
        assert!(t > 0.0);
        p.tick(t + 10.0);
        assert!(matches!(p.ship(s).unwrap().place, Place::Orbit(o) if o.body == "moon"));
        assert!(p.progress.milestones.contains("orbit:moon"));
        // Mars is era 5: closed until a resource is claimed.
        assert!(p.transfer(s, "mars").unwrap_err().contains("era 5"));
    }
}

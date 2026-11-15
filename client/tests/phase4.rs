//! Phase 4 gate, flown through the real sim: Era 1 to a station, refuelling, the Moon and a
//! planet claim, using only parts the research tree has actually unlocked by that point.
use orbital_client::autopilot::{Ascent, Descent};
use orbital_client::program::{FlightResult, Program};
use orbital_client::report::Outcome;
use orbital_client::sim::Flight;
use orbital_client::vessel::Vessel;

fn v(stages: &[&[&'static str]]) -> Vessel {
    Vessel { stages: stages.iter().map(|s| s.to_vec()).collect() }
}

fn need(p: &mut Program, ids: &[&str]) {
    for id in ids {
        if !p.progress.has(id) {
            let why = p.progress.blocker(id);
            assert!(p.progress.unlock(id), "cannot unlock {id} with {} RP: {why:?}", p.progress.rp);
        }
    }
}

fn check_unlocked(p: &Program, ves: &Vessel) {
    for s in 0..ves.stages.len() {
        for part in ves.parts(s) {
            assert!(p.progress.has(part.id), "{} is not unlocked", part.id);
        }
    }
}

fn hop(p: &mut Program) -> FlightResult {
    let ves = v(&[&["spark", "tank_m", "tank_s"], &["pod1", "chute1"]]);
    check_unlocked(p, &ves);
    let t0 = p.time;
    let mut f = Flight::launch(ves, t0);
    f.set_throttle(1.0);
    f.set_pitch(0.0);
    let mut staged = false;
    while !f.status.is_over() && f.t < 3000.0 {
        if f.stage_empty() && !staged { f.stage(); staged = true; }
        if f.alt() < 12_000.0 && f.vel.dot(f.pos) < 0.0 && !f.chute_deployed && f.rel_vel().length() < 300.0 { f.deploy_chute(); }
        f.advance(0.1, 0.05);
    }
    p.finish_flight("hop", &f, t0, &[])
}

fn to_orbit(p: &mut Program, name: &str, ves: Vessel, crew: &[&str]) -> u64 {
    check_unlocked(p, &ves);
    let crew: Vec<String> = crew.iter().map(|c| c.to_string()).collect();
    p.check_launch_crew(&ves, &crew).unwrap();
    let t0 = p.time;
    let mut f = Flight::launch(ves, t0);
    Ascent::new(180_000.0).run(&mut f, 3000.0);
    let r = p.finish_flight(name, &f, t0, &crew);
    assert_eq!(r.report.outcome, Outcome::Orbit, "{name}: {} {:?}", r.report.headline, r.report.diagnosis);
    r.ship.unwrap()
}

fn rocket(top: &[&'static str]) -> Vessel {
    let mut t = vec!["vacuum", "tank_m"];
    t.extend_from_slice(top);
    v(&[
        &["vector", "vector", "vector", "vector", "vector", "vector", "tank_xl", "tank_xl", "tank_xl"],
        &["vector", "tank_l", "tank_l", "tank_l"],
        &t,
    ])
}

#[test]
fn era1_to_first_orbit_has_no_dead_end() {
    let mut p = Program::new();
    p.add_player("ann").unwrap();
    let r = hop(&mut p);
    println!("{} | {:?} | rp {}", r.report.headline, r.awards, p.progress.rp);
    assert_eq!(r.report.outcome, Outcome::SpaceRecovered);
    assert_eq!(p.progress.era(), 2);
    println!("rp after hop {}", p.progress.rp);
    need(&mut p, &["vector", "tank_l", "tank_xl", "vacuum"]);
    println!("rp after unlocks {}", p.progress.rp);
    let id = to_orbit(&mut p, "first", rocket(&["pod1", "chute1"]), &["ann"]);
    println!("rp after orbit {} era {} ship {id}", p.progress.rp, p.progress.era());
    assert_eq!(p.progress.era(), 3);
}

/// Everything up to a finished station with a habitat: returns the program and the station id.
fn build_station() -> (Program, u64) {
    let mut p = Program::new();
    p.add_player("ann").unwrap();
    p.add_player("bob").unwrap();
    hop(&mut p);
    need(&mut p, &["vector", "tank_l", "tank_xl", "vacuum"]);
    to_orbit(&mut p, "first", rocket(&["pod1", "chute1"]), &["ann"]);
    p.teleport_to_earth("ann").unwrap();
    need(&mut p, &["dock1", "core1", "solar1"]);
    let core = to_orbit(&mut p, "core", rocket(&["core1", "dock1", "solar1", "solar1", "pod1"]), &["ann"]);
    need(&mut p, &["hab1", "supply1"]);
    let hab = to_orbit(&mut p, "hab", rocket(&["hab1", "dock1", "supply1", "pod1"]), &["bob"]);
    let st = p.found_station(core, "Alpha").unwrap();
    p.rendezvous(hab, st).unwrap();
    p.attach(hab).unwrap();
    (p, st)
}

/// Launch a module with a pilot from Earth, dock it and fold it into the station.
fn add_module(p: &mut Program, st: u64, pilot: &str, name: &str, module: &[&'static str]) {
    p.teleport_to_earth(pilot).unwrap();
    let mut top = module.to_vec();
    top.push("pod1");
    let id = to_orbit(p, name, rocket(&top), &[pilot]);
    p.rendezvous(id, st).unwrap();
    if p.station(st).unwrap().prop_kg > 0.0 || p.station(st).unwrap().has(|k| matches!(k, orbital_client::parts::Kind::Depot { .. })) {
        // Top the ferry up before folding it in: the first refuelling pays research.
        p.refuel(id).unwrap();
    }
    p.attach(id).unwrap();
}

#[test]
fn station_refuel_and_assemble() {
    let (mut p, st) = build_station();
    assert_eq!(p.progress.era(), 4);
    println!("rp {} t {:.1} h", p.progress.rp, p.time / 3600.0);
    need(&mut p, &["depot1", "asm1"]);
    add_module(&mut p, st, "bob", "depot", &["depot1", "dock1"]);
    add_module(&mut p, st, "ann", "bay", &["asm1", "dock1"]);
    println!("rp {} t {:.1} h; supplies {:.1} h", p.progress.rp, p.time / 3600.0, p.station(st).unwrap().supply_hours());
    need(&mut p, &["descent1", "survey1"]);
    // A lunar ship built at the bay: a transfer stage and a lander.
    let ves = v(&[&["vacuum", "tank_xl", "tank_xl"], &["descent1", "tank_l", "pod1", "survey1", "dock1", "supply1"]]);
    let ship = p.assemble(st, "Selene", ves).unwrap();
    p.move_crew("ann", orbital_client::program::Loc::Ship(ship)).unwrap();
    let before = p.ship(ship).unwrap().dv_remaining();
    let added = p.refuel(ship).unwrap();
    println!("refuelled {added:.0} kg: dv {before:.0} -> {:.0}", p.ship(ship).unwrap().dv_remaining());
    assert!(p.progress.milestones.contains("refuel"));
    p.undock(ship).unwrap();
    let eta = p.transfer(ship, "moon").unwrap();
    p.tick(eta + 60.0);
    assert!(p.progress.milestones.contains("orbit:moon"), "{:?}", p.log.last());
    println!("at the Moon, t {:.1} h, rp {}, ship {:?}", p.time / 3600.0, p.progress.rp, p.ship(ship).unwrap().place);
    // Powered descent through the real sim, then survey for the claim.
    let (mut f, t0) = p.begin_descent(ship).unwrap();
    Descent::new().run(&mut f, 6_000.0);
    let r = p.finish_descent(ship, &f, t0).unwrap();
    println!("{} {:?}", r.report.headline, r.awards);
    assert_eq!(r.report.outcome, Outcome::Landed, "{:?}", r.report.diagnosis);
    println!("{:#?}", &p.log[p.log.len().saturating_sub(8)..]);
    let lines = p.survey(ship).unwrap();
    println!("{lines:?} claims {:?} era {} rp {}", p.progress.claims, p.progress.era(), p.progress.rp);
    assert!(p.progress.claims.contains("water_ice") || !p.progress.claims.is_empty());
    assert_eq!(p.progress.era(), 5);
    println!("propellant left on the Moon: {:.0} kg, dv {:.0}", p.ship(ship).unwrap().prop, p.ship(ship).unwrap().dv_remaining());
    // Lift off again with what is left and meet the station? Report only; the rescue is a teleport.
    let (mut up, t1) = p.begin_takeoff(ship).unwrap();
    orbital_client::autopilot::Takeoff::new(60_000.0).run(&mut up, 3_000.0);
    let r = p.finish_ascent(ship, &up, t1).unwrap();
    println!("lift-off: {}", r.report.headline);
    // The lander cannot come home on what is left, and says so; the crew is brought back by teleport.
    let err = p.transfer(ship, "earth").unwrap_err();
    assert!(err.contains("short"), "{err}");
    p.teleport_to_player("ann", "bob").unwrap(); // a friend joins bob at the station
    assert_eq!(p.locate("ann"), orbital_client::program::Loc::Station(st));
    // Mars: a planet claim from orbit, flown by a player.
    println!("rp {} before Mars", p.progress.rp);
    let mars = v(&[&["vacuum", "tank_xl", "tank_xl", "tank_xl"], &["vacuum", "tank_xl", "pod1", "survey1", "dock1", "supply1", "supply1"]]);
    let m = p.assemble(st, "Ares", mars).unwrap();
    p.move_crew("bob", orbital_client::program::Loc::Ship(m)).unwrap();
    p.undock(m).unwrap();
    let eta = p.transfer(m, "mars").unwrap();
    p.tick(eta + 60.0);
    let lines = p.survey(m).unwrap();
    println!("{lines:?} claims {:?}", p.progress.claims);
    assert!(p.progress.claims.contains("carbon"));
    // The claims pay off in parts nobody could build before.
    assert!(p.progress.unlock("isru1") && p.progress.unlock("nerva"), "rp {}", p.progress.rp);
    assert!(!p.log.iter().any(|l| l.contains("died")), "{:#?}", p.log);
}

/// Every part and every body can be reached from a fresh program: no unlock waits on something
/// that cannot be earned.
#[test]
fn the_tech_tree_has_no_dead_ends() {
    use orbital_client::{bodies, parts, progress::Progress};
    let mut p = Progress::new();
    // Eras open by milestones that the game awards; resources open by surveying bodies in reach.
    for m in ["space_recovered", "orbit", "station"] {
        p.milestones.insert(m);
    }
    p.rp = 1_000_000;
    let mut open: Vec<_> = bodies::bodies().iter().filter(|b| b.min_era <= 5).collect();
    open.retain(|b| !b.resources.is_empty());
    for b in &open {
        p.claim_all(&b.resources);
    }
    assert_eq!(p.era(), 5);
    let mut progressed = true;
    while progressed {
        progressed = false;
        for part in parts::catalog() {
            if !p.has(part.id) && p.unlock(part.id) {
                progressed = true;
            }
        }
    }
    let stuck: Vec<_> = parts::catalog().iter().filter(|x| !p.has(x.id)).map(|x| (x.id, p.blocker(x.id))).collect();
    assert!(stuck.is_empty(), "{stuck:?}");
    // Every claim a part needs is a resource some body actually has.
    let have: std::collections::BTreeSet<_> = bodies::bodies().iter().flat_map(|b| b.resources.iter().copied()).collect();
    for part in parts::catalog() {
        if let Some(c) = part.claim {
            assert!(have.contains(c), "{} needs {c}, which no body has", part.id);
        }
    }
}

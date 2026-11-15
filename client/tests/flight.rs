use orbital_client::autopilot::Ascent;
use orbital_client::report::{FlightReport, Outcome};
use orbital_client::sim::{Flight, Status};
use orbital_client::vessel::Vessel;

fn v(stages: &[&[&'static str]]) -> Vessel {
    Vessel { stages: stages.iter().map(|s| s.to_vec()).collect() }
}

fn orbiter() -> Vessel {
    v(&[
        &["vector", "vector", "vector", "vector", "tank_xl", "tank_xl"],
        &["vector", "tank_l", "tank_l"],
        &["vacuum", "tank_m", "sat1", "pod1", "chute1", "cone1"],
    ])
}

#[test]
fn era1_hop_reaches_space_and_recovers() {
    let mut f = Flight::new(v(&[&["spark", "tank_m", "tank_s"], &["pod1", "chute1", "cone1"]]));
    f.set_throttle(1.0);
    f.set_pitch(0.0);
    let mut staged = false;
    while !f.status.is_over() && f.t < 3000.0 {
        if f.stage_empty() && !staged { f.stage(); staged = true; }
        if f.alt() < 12_000.0 && f.vel.dot(f.pos) < 0.0 && !f.chute_deployed && f.rel_vel().length() < 300.0 { f.deploy_chute(); }
        f.advance(0.1, 0.05);
    }
    let r = FlightReport::from_flight(&f);
    println!("{}\n{:#?}\n{:#?}", r.headline, r.diagnosis, r.timeline);
    assert!(f.reached_space, "{}", r.headline);
    assert_eq!(r.outcome, Outcome::SpaceRecovered, "{}", r.headline);
}

#[test]
fn orbit_is_reachable() {
    let ves = orbiter();
    for (i, s) in ves.stats().iter().enumerate() {
        println!("stage {} m0 {:.0} dv {:.0} twr {:.2}", i + 1, s.m0, s.dv_vac, s.twr_sl);
    }
    println!("total dv {:.0}", ves.total_dv());
    let mut f = Flight::new(ves);
    Ascent::new(180_000.0).run(&mut f, 3000.0);
    let r = FlightReport::from_flight(&f);
    println!("{}\n{:#?}\n{:#?}", r.headline, r.diagnosis, r.timeline);
    assert!(f.reached_orbit, "{} / status {:?}", r.headline, f.status);
    assert!(f.t < 3600.0);
}

#[test]
fn failures_explain_themselves() {
    // Too little thrust
    let mut f = Flight::new(v(&[&["spark", "tank_m", "tank_m", "tank_m", "tank_m"], &["pod1"]]));
    f.set_throttle(1.0);
    f.advance(10.0, 0.05);
    f.end();
    let r = FlightReport::from_flight(&f);
    assert_eq!(r.outcome, Outcome::NeverLaunched);
    assert!(r.diagnosis[0].contains("thrust-to-weight"), "{:?}", r.diagnosis);

    // Straight up at full throttle: max-Q failure
    let mut f = Flight::new(v(&[&["vector", "vector", "tank_l"], &["pod1", "chute1"]]));
    f.set_throttle(1.0);
    f.advance(120.0, 0.05);
    let r = FlightReport::from_flight(&f);
    println!("{}\n{:#?}", r.headline, r.diagnosis);
    assert!(matches!(f.status, Status::BrokeUp { .. }), "{:?}", f.status);
    assert!(r.diagnosis[0].contains("Max-Q"));

    // Era 1 hop with no chute: crash explained
    let mut f = Flight::new(v(&[&["spark", "tank_m"], &["pod1"]]));
    f.set_throttle(1.0);
    while !f.status.is_over() && f.t < 1500.0 { if f.stage_empty() { f.stage(); } f.advance(0.1, 0.05); }
    let r = FlightReport::from_flight(&f);
    println!("{}\n{:#?}", r.headline, r.diagnosis);
    assert_eq!(r.outcome, Outcome::Crashed);
    assert!(r.diagnosis.iter().any(|d| d.contains("parachute")));
}

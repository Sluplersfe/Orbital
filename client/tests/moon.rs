//! Landing on the Moon and coming back, flown through the real 64-bit sim.
use orbital_client::autopilot::{Descent, Takeoff};
use orbital_client::bodies::{self, moon};
use orbital_client::report::{FlightReport, Outcome};
use orbital_client::sim::{Flight, Status};
use orbital_client::space::{Orbit, Place, Spacecraft};
use orbital_client::vessel::Vessel;

fn lander() -> Vessel {
    Vessel { stages: vec![vec!["descent1", "tank_l", "pod1", "survey1", "dock1"]] }
}

fn in_lunar_orbit(v: Vessel) -> Spacecraft {
    let m = moon();
    Spacecraft::new(1, "lander", v, Place::Orbit(Orbit { body: "moon", r: m.park_radius(), theta0: 0.3, epoch: 0.0 }))
}

#[test]
fn lander_descends_and_touches_down() {
    let sc = in_lunar_orbit(lander());
    let mut f = sc.flight_in_orbit(0.0).unwrap();
    Descent::new().run(&mut f, 6_000.0);
    let r = FlightReport::from_flight(&f);
    println!("{}\n{:#?}\n{:#?}", r.headline, r.diagnosis, r.timeline);
    assert!(matches!(f.status, Status::Landed { .. }), "{:?} {}", f.status, r.headline);
    assert_eq!(r.outcome, Outcome::Landed);
    // The landing costs about what the planner says, and leaves propellant.
    let budget = bodies::landing_dv(moon(), moon().park_radius());
    assert!(f.dv_used < budget * 1.3, "used {} budget {budget}", f.dv_used);
    assert!(f.prop > 0.0);
}

#[test]
fn a_lander_without_the_delta_v_crashes_and_the_report_says_why() {
    let v = Vessel { stages: vec![vec!["descent1", "tank_s", "crew1", "survey1", "dock1", "sat1", "sat1"]] };
    let sc = in_lunar_orbit(v);
    let mut f = sc.flight_in_orbit(0.0).unwrap();
    Descent::new().run(&mut f, 6_000.0);
    let r = FlightReport::from_flight(&f);
    println!("{}\n{:#?}", r.headline, r.diagnosis);
    assert!(matches!(f.status, Status::Crashed { .. }), "{:?}", f.status);
    assert_eq!(r.outcome, Outcome::Crashed);
    assert!(r.diagnosis.iter().any(|d| d.contains("ran dry") || d.contains("never lit")), "{:?}", r.diagnosis);
}

#[test]
fn a_landed_ship_can_lift_off_and_reach_lunar_orbit() {
    let sc = in_lunar_orbit(Vessel { stages: vec![vec!["descent1", "tank_l", "pod1"]] });
    let mut f = sc.flight_in_orbit(0.0).unwrap();
    Descent::new().run(&mut f, 6_000.0);
    assert!(matches!(f.status, Status::Landed { .. }));
    // Not enough left to come home? The lander landed with some propellant; refuel, then launch.
    let mut up = Flight::on_surface(moon(), f.vessel.clone(), f.stage, f.vessel.stage_prop(0), 0.2);
    Takeoff::new(60_000.0).run(&mut up, 3_000.0);
    let r = FlightReport::from_flight(&up);
    println!("{}\n{:#?}", r.headline, r.diagnosis);
    assert!(up.reached_orbit, "{} {:?}", r.headline, up.status);
    assert!(up.peri_alt() >= moon().min_orbit_alt());
}

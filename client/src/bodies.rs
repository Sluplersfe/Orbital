//! Bodies and patched-conic transfer planning. The table lives in `data/bodies.tsv`.
//!
//! Each ship is under one body's gravity at a time. A transfer between bodies is planned as a
//! chain of impulsive burns (Hohmann in the common parent's frame, with escape and capture burns
//! at each end). Delta-v is physical; the transit *time* is a designed value per body because the
//! world clock has no time warp (see PLAN.md).
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurveyFrom {
    Surface,
    Orbit,
}

#[derive(Debug, Clone)]
pub struct Body {
    pub id: &'static str,
    pub name: &'static str,
    pub parent: Option<&'static str>,
    pub mu: f64,
    pub radius: f64,
    /// Semi-major axis of the (circular) orbit about the parent (m).
    pub sma: f64,
    pub omega: f64,
    pub rho0: f64,
    pub scale_h: f64,
    pub atmo_top: f64,
    pub transit_s: f64,
    pub survey: SurveyFrom,
    /// Default low parking orbit altitude (m).
    pub park_alt: f64,
    /// Era in which crews may survey and claim here (Moon and asteroids: 4, planets: 5).
    pub min_era: u8,
    pub resources: Vec<&'static str>,
}

impl Body {
    pub fn has_atmosphere(&self) -> bool {
        self.rho0 > 0.0
    }

    /// Altitude counted as "space" over this body: the top of the atmosphere, or 10 km for an
    /// airless body (a 100 km Karman line for Earth).
    pub fn space_alt(&self) -> f64 {
        if self.id == "earth" { crate::SPACE_ALT } else if self.has_atmosphere() { self.atmo_top } else { 10_000.0 }
    }

    /// Lowest altitude of a stable orbit: above the air, or a few km over airless terrain.
    pub fn min_orbit_alt(&self) -> f64 {
        if self.id == "earth" { crate::sim::ORBIT_PERIAPSIS } else if self.has_atmosphere() { self.atmo_top + 10_000.0 } else { 15_000.0 }
    }

    pub fn park_radius(&self) -> f64 {
        self.radius + self.park_alt
    }

    pub fn circular_speed(&self, r: f64) -> f64 {
        (self.mu / r).sqrt()
    }

    pub fn surface_gravity(&self) -> f64 {
        self.mu / (self.radius * self.radius)
    }

    /// Sphere-of-influence radius about the parent (m); infinite for the root.
    pub fn soi(&self) -> f64 {
        match self.parent {
            Some(p) => self.sma * (self.mu / body(p).mu).powf(0.4),
            None => f64::INFINITY,
        }
    }
}

static BODIES: OnceLock<Vec<Body>> = OnceLock::new();

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

pub fn bodies() -> &'static [Body] {
    BODIES.get_or_init(|| {
        include_str!("../data/bodies.tsv")
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| {
                let c: Vec<&str> = l.split_whitespace().collect();
                assert_eq!(c.len(), 15, "bodies.tsv: bad row {l:?}");
                let n = |i: usize| -> f64 { c[i].parse().unwrap_or_else(|_| panic!("bodies.tsv: bad number {:?} in {l:?}", c[i])) };
                Body {
                    id: leak(c[0]),
                    name: leak(c[1]),
                    parent: (c[2] != "-").then(|| leak(c[2])),
                    mu: n(3), radius: n(4), sma: n(5), omega: n(6), rho0: n(7), scale_h: n(8), atmo_top: n(9),
                    transit_s: n(10),
                    survey: if c[11] == "orbit" { SurveyFrom::Orbit } else { SurveyFrom::Surface },
                    park_alt: n(12),
                    min_era: n(13) as u8,
                    resources: if c[14] == "-" { vec![] } else { c[14].split(',').map(leak).collect() },
                }
            })
            .collect()
    })
}

pub fn get(id: &str) -> Option<&'static Body> {
    bodies().iter().find(|b| b.id == id)
}

/// Like `get`, but for ids that must exist.
pub fn body(id: &str) -> &'static Body {
    get(id).unwrap_or_else(|| panic!("unknown body {id}"))
}

pub fn earth() -> &'static Body {
    body("earth")
}

pub fn moon() -> &'static Body {
    body("moon")
}

/// `id` then its ancestors, root last.
fn chain(id: &str) -> Vec<&'static Body> {
    let mut v = vec![];
    let mut cur = Some(body(id));
    while let Some(b) = cur {
        v.push(b);
        cur = b.parent.map(body);
    }
    v
}

#[derive(Debug, Clone, PartialEq)]
pub struct Leg {
    pub label: String,
    pub dv: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub legs: Vec<Leg>,
    pub total_dv: f64,
    /// Designed world seconds the trip takes.
    pub transit_s: f64,
    /// Burn that leaves the origin orbit (legs before arrival).
    pub departure_dv: f64,
    /// Burn that captures at the destination.
    pub arrival_dv: f64,
}

fn vc(mu: f64, r: f64) -> f64 {
    (mu / r).sqrt()
}

/// Hohmann between circular co-planar orbits of radius a1 and a2 about mu: (burn 1, burn 2).
pub fn hohmann(mu: f64, a1: f64, a2: f64) -> (f64, f64) {
    let at = (a1 + a2) / 2.0;
    let v1 = (mu * (2.0 / a1 - 1.0 / at)).sqrt();
    let v2 = (mu * (2.0 / a2 - 1.0 / at)).sqrt();
    ((v1 - vc(mu, a1)).abs(), (vc(mu, a2) - v2).abs())
}

/// Burn from a circular orbit r about mu to leave with excess speed `vinf` (0 = just escape).
fn escape_burn(mu: f64, r: f64, vinf: f64) -> f64 {
    (vinf * vinf + 2.0 * mu / r).sqrt() - vc(mu, r)
}

/// Delta-v to fly from a circular orbit of radius `r_src` about `src` to a circular orbit of radius
/// `r_dst` about `dst` (patched conics, impulsive burns, coplanar circular body orbits).
pub fn route(src: &str, r_src: f64, dst: &str, r_dst: f64) -> Route {
    let (s, d) = (body(src), body(dst));
    let mut legs: Vec<Leg> = vec![];
    if s.id == d.id {
        let (a, b) = hohmann(s.mu, r_src, r_dst);
        legs.push(Leg { label: format!("Change {} orbit, burn 1", s.name), dv: a });
        legs.push(Leg { label: format!("Change {} orbit, burn 2", s.name), dv: b });
        let total_dv = a + b;
        return Route { legs, total_dv, transit_s: 600.0, departure_dv: a, arrival_dv: b };
    }
    let (cs, cd) = (chain(src), chain(dst));
    let common = *cs.iter().find(|b| cd.iter().any(|x| x.id == b.id)).expect("one solar system");
    // Climb from src to the child of `common` (or `common` itself).
    let mut x = s;
    let mut r = r_src;
    let mut up: Vec<Leg> = vec![];
    while x.id != common.id && x.parent != Some(common.id) {
        up.push(Leg { label: format!("Escape {}", x.name), dv: escape_burn(x.mu, r, 0.0) });
        r = x.sma;
        x = body(x.parent.unwrap());
    }
    // Descend side, built from dst upward, then reversed.
    let mut y = d;
    let mut ry = r_dst;
    let mut down: Vec<(&'static Body, f64)> = vec![]; // (body, orbit radius reached in it)
    while y.id != common.id && y.parent != Some(common.id) {
        down.push((y, ry));
        ry = y.sma;
        y = body(y.parent.unwrap());
    }
    // Middle: Hohmann in `common`'s frame between circular orbits a1 (x's side) and a2 (y's side).
    let a1 = if x.id == common.id { r } else { x.sma };
    let a2 = if y.id == common.id { ry } else { y.sma };
    let (b1, b2) = hohmann(common.mu, a1, a2);
    legs.extend(up);
    if x.id == common.id {
        legs.push(Leg { label: format!("Transfer burn from {} orbit", common.name), dv: b1 });
    } else {
        legs.push(Leg { label: format!("Depart {} for {}", x.name, y.name), dv: escape_burn(x.mu, r, b1) });
    }
    let n_depart = legs.len();
    // Capture at y: enter its sphere of influence with excess speed b2 into a circular orbit of
    // radius ry (the destination orbit, or the co-orbit of the next body down the chain).
    let arrival = if y.id == common.id { b2 } else { escape_burn(y.mu, ry, b2) };
    legs.push(Leg { label: format!("Capture at {}", y.name), dv: arrival });
    for (b, rb) in down.iter().rev() {
        // From co-orbit with b (zero excess speed) into a circular orbit of radius rb about b.
        legs.push(Leg { label: format!("Capture at {}", b.name), dv: escape_burn(b.mu, *rb, 0.0) });
    }
    let departure: f64 = legs.iter().take(n_depart).map(|l| l.dv).sum();
    let total_dv: f64 = legs.iter().map(|l| l.dv).sum();
    let transit_s = (d.transit_s - s.transit_s).abs() + 300.0;
    Route { legs, total_dv, transit_s, departure_dv: departure, arrival_dv: total_dv - departure }
}

/// Delta-v from a body's default parking orbit to another body's default parking orbit.
pub fn route_between(src: &str, dst: &str) -> Route {
    route(src, body(src).park_radius(), dst, body(dst).park_radius())
}

/// Delta-v of an airless powered descent from a circular orbit to the surface, with gravity losses
/// (suicide burn): orbital speed at the park radius plus a hover allowance.
pub fn landing_dv(b: &Body, r_orbit: f64) -> f64 {
    let transfer = hohmann(b.mu, r_orbit, b.radius + 12_000.0).0; // lower periapsis
    let v_peri = (b.mu * (2.0 / (b.radius + 12_000.0) - 2.0 / (r_orbit + b.radius + 12_000.0))).sqrt();
    transfer + v_peri + 0.15 * b.surface_gravity() * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_loads() {
        assert!(bodies().len() >= 10);
        let e = earth();
        assert_eq!(e.radius, crate::R_EARTH);
        assert_eq!(e.mu, crate::MU);
        assert_eq!(e.omega, crate::sim::OMEGA);
        assert!(moon().soi() > 5e7 && moon().soi() < 7.5e7, "{}", moon().soi());
    }

    #[test]
    fn invented_planets_are_beyond_neptune() {
        let n = body("neptune").sma;
        for id in ["tycho", "ymir"] {
            assert!(body(id).sma > n, "{id}");
        }
        assert_eq!(body("halla").parent, Some("ymir"));
    }

    #[test]
    fn earth_to_moon_costs_what_it_should() {
        let r = route_between("earth", "moon");
        let tli = r.legs[0].dv;
        assert!((3_000.0..3_300.0).contains(&tli), "TLI {tli}");
        // Capture into a 100 km lunar orbit is roughly 0.8-1.0 km/s.
        assert!((700.0..1_100.0).contains(&r.arrival_dv), "{r:?}");
        assert!((r.total_dv - 4_000.0).abs() < 400.0, "{}", r.total_dv);
    }

    #[test]
    fn earth_to_mars_costs_what_it_should() {
        let r = route_between("earth", "mars");
        assert!((3_400.0..3_900.0).contains(&r.departure_dv), "{r:?}");
        assert!((1_900.0..2_700.0).contains(&r.arrival_dv), "{r:?}");
    }

    #[test]
    fn deeper_targets_cost_more() {
        let mars = route_between("earth", "mars").total_dv;
        let vesta = route_between("earth", "vesta").total_dv;
        let ymir = route_between("earth", "ymir").total_dv;
        assert!(vesta > 0.0 && mars > 0.0 && ymir > mars, "{vesta} {mars} {ymir}");
        // A moon of an outer planet is reached by a cheap capture into a wide orbit of its planet.
        let halla = route_between("earth", "halla");
        assert!(halla.total_dv > mars && halla.legs.len() == 3, "{halla:?}");
    }

    #[test]
    fn routes_are_roughly_symmetric_through_the_tree() {
        let a = route_between("earth", "mars").total_dv;
        let b = route_between("mars", "earth").total_dv;
        assert!((a - b).abs() / a < 0.5, "{a} {b}");
        let moon_mars = route_between("moon", "mars");
        assert!(moon_mars.legs.len() >= 3 && moon_mars.total_dv > 0.0);
    }

    #[test]
    fn landing_budget_is_sane() {
        let m = moon();
        let dv = landing_dv(m, m.park_radius());
        assert!((1_700.0..2_200.0).contains(&dv), "{dv}");
    }
}

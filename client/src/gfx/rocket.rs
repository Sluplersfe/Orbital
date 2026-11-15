//! A 3D model of a vessel, built from primitive shapes. The same part definitions that drive the
//! physics decide the sizes: tank volume from propellant mass, bell size from thrust.
use bevy::prelude::*;
use orbital_client::parts::{self, Kind};
use orbital_client::vessel::Vessel;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Mat {
    White,
    Black,
    Metal,
    Dark,
    Gold,
    Orange,
    Solar,
    Glass,
    Green,
}

const MATS: [Mat; 9] = [Mat::White, Mat::Black, Mat::Metal, Mat::Dark, Mat::Gold, Mat::Orange, Mat::Solar, Mat::Glass, Mat::Green];

/// Shared materials for rocket parts, plain and highlighted (the builder's selected stage).
#[derive(Resource)]
pub struct Kit {
    plain: Vec<Handle<StandardMaterial>>,
    lit: Vec<Handle<StandardMaterial>>,
    pub plume: Handle<StandardMaterial>,
}

impl Kit {
    pub fn new(materials: &mut Assets<StandardMaterial>) -> Self {
        let base = |m: Mat| -> StandardMaterial {
            let (c, metal, rough) = match m {
                Mat::White => (Color::srgb(0.9, 0.9, 0.88), 0.0, 0.45),
                Mat::Black => (Color::srgb(0.05, 0.05, 0.06), 0.0, 0.5),
                Mat::Metal => (Color::srgb(0.6, 0.6, 0.62), 0.9, 0.35),
                Mat::Dark => (Color::srgb(0.18, 0.18, 0.2), 0.6, 0.5),
                Mat::Gold => (Color::srgb(0.85, 0.65, 0.2), 1.0, 0.3),
                Mat::Orange => (Color::srgb(0.95, 0.45, 0.1), 0.0, 0.6),
                Mat::Solar => (Color::srgb(0.08, 0.12, 0.35), 0.4, 0.2),
                Mat::Glass => (Color::srgb(0.05, 0.08, 0.1), 0.2, 0.05),
                Mat::Green => (Color::srgb(0.3, 0.7, 0.35), 0.0, 0.4),
            };
            StandardMaterial { base_color: c, metallic: metal, perceptual_roughness: rough, ..default() }
        };
        let plain = MATS.iter().map(|&m| materials.add(base(m))).collect();
        let lit = MATS.iter().map(|&m| materials.add(StandardMaterial { emissive: LinearRgba::rgb(0.05, 0.25, 0.9), ..base(m) })).collect();
        let plume = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.7, 0.3, 0.55),
            emissive: LinearRgba::rgb(40.0, 18.0, 5.0),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        });
        Self { plain, lit, plume }
    }

    pub fn mat(&self, m: Mat, lit: bool) -> Handle<StandardMaterial> {
        let i = MATS.iter().position(|&x| x == m).unwrap_or(0);
        if lit { self.lit[i].clone() } else { self.plain[i].clone() }
    }
}

pub struct Piece {
    pub stage: usize,
    pub mesh: Mesh,
    pub mat: Mat,
    pub tf: Transform,
}

/// Where an engine's exhaust leaves: bottom-centre of the bell, its radius, and its stage.
#[derive(Clone, Copy)]
pub struct Nozzle {
    pub stage: usize,
    pub at: Vec3,
    pub radius: f32,
}

pub struct Layout {
    pub pieces: Vec<Piece>,
    pub nozzles: Vec<Nozzle>,
    pub height: f32,
    /// Height of the pilot's eye: the centre of the top pod (or the top, for a probe).
    pub eye: f32,
}

fn tank_dims(prop_kg: f64) -> (f32, f32) {
    let v = (prop_kg / 1000.0) as f32;
    let r = 0.42 * v.cbrt();
    (r, v / (std::f32::consts::PI * r * r))
}

fn bell(thrust_kn: f64, isp_sl: f64, isp_vac: f64) -> (f32, f32) {
    let mut rb = (0.28 * (thrust_kn as f32 / 100.0).sqrt()).clamp(0.16, 1.4);
    let vacuum = isp_vac / isp_sl.max(1.0) > 1.6;
    if vacuum {
        rb *= 1.5;
    }
    let hb = rb * if vacuum { 1.7 } else { 1.3 };
    (rb, hb)
}

fn rank(k: &Kind) -> u8 {
    match k {
        Kind::Engine { .. } => 0,
        Kind::Structure => 1,
        Kind::Tank { .. } | Kind::Depot { .. } => 2,
        Kind::Battery { .. } | Kind::Core | Kind::Habitat { .. } | Kind::Farm { .. } | Kind::Isru { .. } | Kind::Assembly => 3,
        Kind::Supply { .. } | Kind::Payload | Kind::Survey | Kind::Solar { .. } => 4,
        Kind::Pod { .. } => 6,
        Kind::Docking => 7,
        Kind::Parachute => 8,
        Kind::NoseCone => 9,
        Kind::Node { .. } => 10,
    }
}

fn stage_radius(v: &Vessel, s: usize) -> f32 {
    let r = v.parts(s).map(|p| match p.kind {
        Kind::Tank { prop_kg } => tank_dims(prop_kg).0,
        Kind::Depot { .. } | Kind::Habitat { .. } | Kind::Farm { .. } => 2.0,
        Kind::Core | Kind::Isru { .. } => 1.6,
        Kind::Pod { crew } => if crew > 1 { 1.4 } else { 0.95 },
        _ => 0.0,
    }).fold(0.0f32, f32::max);
    if r == 0.0 { 0.7 } else { r }
}

fn cyl(r: f32, h: f32) -> Mesh {
    Cylinder::new(r, h).mesh().resolution(32).build()
}

fn frustum(bottom: f32, top: f32, h: f32) -> Mesh {
    ConicalFrustum { radius_top: top, radius_bottom: bottom, height: h }.mesh().resolution(32).build()
}

/// Lay out the vessel from `from_stage` up, base at y = 0.
pub fn layout(v: &Vessel, from_stage: usize) -> Layout {
    let mut pieces = vec![];
    let mut nozzles = vec![];
    let mut y = 0.0f32;
    let mut eye = None;
    let n = v.stages.len();
    for s in from_stage..n {
        let sr = stage_radius(v, s);
        let mut ps: Vec<&parts::Part> = v.parts(s).collect();
        ps.sort_by_key(|p| rank(&p.kind));
        let mut add = |mesh: Mesh, mat: Mat, tf: Transform| pieces.push(Piece { stage: s, mesh, mat, tf });

        // Engines, clustered in a ring when there are several.
        let engines: Vec<_> = ps.iter().filter_map(|p| match p.kind { Kind::Engine { thrust_kn, isp_sl, isp_vac } => Some((thrust_kn, isp_sl, isp_vac)), _ => None }).collect();
        let mut he = 0.0f32;
        let k = engines.len();
        for (i, &(t, sl, vac)) in engines.iter().enumerate() {
            let (rb, hb) = bell(t, sl, vac);
            let (x, z) = if k == 1 { (0.0, 0.0) } else {
                let a = i as f32 / k as f32 * std::f32::consts::TAU;
                let ring = (sr - rb).max(rb * 1.05);
                (a.cos() * ring, a.sin() * ring)
            };
            add(frustum(rb, rb * 0.35, hb), Mat::Dark, Transform::from_xyz(x, y + hb / 2.0, z));
            let body_h = hb * 0.5;
            add(cyl(rb * 0.45, body_h), Mat::Metal, Transform::from_xyz(x, y + hb + body_h / 2.0, z));
            nozzles.push(Nozzle { stage: s, at: Vec3::new(x, y, z), radius: rb });
            he = he.max(hb + body_h);
        }
        if k > 0 && s == from_stage {
            // Thrust plate over the engines of the bottom stage.
            add(frustum(sr * 0.9, sr, 0.25), Mat::Dark, Transform::from_xyz(0.0, y + he + 0.125, 0.0));
            he += 0.25;
        }
        y += he;

        for p in ps.iter().filter(|p| !matches!(p.kind, Kind::Engine { .. } | Kind::Node { .. })) {
            match p.kind {
                Kind::Structure => {
                    let h = 0.35;
                    add(cyl(sr * 1.03, h), Mat::Metal, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    y += h;
                }
                Kind::Tank { prop_kg } => {
                    let (r, h) = tank_dims(prop_kg);
                    add(cyl(r, h), Mat::White, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    for by in [y + 0.12, y + h - 0.12] {
                        add(cyl(r * 1.01, 0.16), Mat::Black, Transform::from_xyz(0.0, by, 0.0));
                    }
                    y += h;
                }
                Kind::Depot { .. } => {
                    let h = 4.5;
                    add(cyl(2.0, h), Mat::Metal, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    add(cyl(2.03, 0.3), Mat::Orange, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    y += h;
                }
                Kind::Core => {
                    let h = 3.0;
                    add(cyl(1.6, h), Mat::White, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    for a in 0..4 {
                        let ang = a as f32 * std::f32::consts::FRAC_PI_2;
                        add(cyl(0.5, 0.6), Mat::Metal, Transform::from_xyz(ang.cos() * 1.8, y + h / 2.0, ang.sin() * 1.8).with_rotation(Quat::from_rotation_arc(Vec3::Y, Vec3::new(ang.cos(), 0.0, ang.sin()))));
                    }
                    y += h;
                }
                Kind::Habitat { crew, .. } => {
                    let h = if crew > 4 { 8.0 } else { 5.0 };
                    add(cyl(2.0, h), Mat::White, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    add(cyl(2.02, 0.5), Mat::Glass, Transform::from_xyz(0.0, y + h * 0.6, 0.0));
                    y += h;
                }
                Kind::Farm { .. } => {
                    let h = 4.0;
                    add(cyl(2.0, h), Mat::White, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    add(cyl(2.02, h * 0.6), Mat::Green, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    y += h;
                }
                Kind::Isru { .. } => {
                    let h = 3.0;
                    add(cyl(1.6, h), Mat::Orange, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    y += h;
                }
                Kind::Assembly => {
                    let h = 4.0;
                    add(Cuboid::new(3.2, h, 3.2).into(), Mat::Metal, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    add(Cuboid::new(3.0, h * 0.8, 3.3).into(), Mat::Dark, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    y += h;
                }
                Kind::Battery { .. } => {
                    let h = 0.6;
                    add(cyl(sr.min(1.2), h), Mat::Dark, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    y += h;
                }
                Kind::Solar { .. } => {
                    let h = 0.8;
                    add(cyl(0.5, h), Mat::Metal, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    for side in [-1.0f32, 1.0] {
                        add(Cuboid::new(6.0, 0.05, 1.6).into(), Mat::Solar, Transform::from_xyz(side * 3.6, y + h / 2.0, 0.0));
                    }
                    y += h;
                }
                Kind::Supply { .. } => {
                    let h = 1.6;
                    add(cyl(0.9, h), Mat::Metal, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    y += h;
                }
                Kind::Payload => {
                    let h = 1.0;
                    add(Cuboid::new(1.0, h, 1.0).into(), Mat::Gold, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    for side in [-1.0f32, 1.0] {
                        add(Cuboid::new(2.4, 0.04, 0.9).into(), Mat::Solar, Transform::from_xyz(side * 1.8, y + h / 2.0, 0.0));
                    }
                    y += h;
                }
                Kind::Survey => {
                    let h = 0.6;
                    add(cyl(0.6, h), Mat::Metal, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    add(Cone::new(0.5, 0.3).into(), Mat::White, Transform::from_xyz(0.85, y + h / 2.0, 0.0).with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)));
                    y += h;
                }
                Kind::Pod { crew } => {
                    let (r, h) = if crew > 1 { (1.4, 2.0) } else { (0.95, 1.5) };
                    add(frustum(r, r * 0.38, h), Mat::White, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    // Two windows facing the pilot's view (+Z side of the model).
                    for side in [-1.0f32, 1.0] {
                        let wy = y + h * 0.55;
                        let wr = r * 0.72;
                        add(Cuboid::new(0.22, 0.26, 0.04).into(), Mat::Glass, Transform::from_xyz(side * 0.22 * r, wy, wr * 0.95).with_rotation(Quat::from_rotation_x(-0.42)));
                    }
                    eye = Some(y + h * 0.5);
                    y += h;
                }
                Kind::Docking => {
                    let h = 0.35;
                    add(cyl(0.55, h), Mat::Metal, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    y += h;
                }
                Kind::Parachute => {
                    let h = 0.35;
                    add(cyl(0.33, h), Mat::Orange, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    y += h;
                }
                Kind::NoseCone => {
                    let h = sr * 2.2;
                    add(Cone::new(sr, h).into(), Mat::White, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                    y += h;
                }
                Kind::Engine { .. } | Kind::Node { .. } => {}
            }
        }

        // Interstage: a skirt from this stage up to the next one's radius, hiding its engines.
        if s + 1 < n {
            let next_engines = v.parts(s + 1).filter_map(|p| match p.kind { Kind::Engine { thrust_kn, isp_sl, isp_vac } => Some(bell(thrust_kn, isp_sl, isp_vac)), _ => None });
            let h = next_engines.map(|(_, hb)| hb * 1.5).fold(0.0f32, f32::max);
            if h > 0.0 {
                let top = stage_radius(v, s + 1);
                add(frustum(sr, top, h), Mat::White, Transform::from_xyz(0.0, y + h / 2.0, 0.0));
                add(cyl(sr.min(top) * 1.01 + 0.01, 0.12), Mat::Black, Transform::from_xyz(0.0, y + h - 0.06, 0.0));
            }
        }
    }
    Layout { pieces, nozzles, height: y, eye: eye.unwrap_or(y) }
}

/// Marks an exhaust plume; `len` is its length at full throttle.
#[derive(Component)]
pub struct Plume {
    pub len: f32,
    pub at: Vec3,
    pub radius: f32,
}

/// Spawn the model of `v` from `from_stage` up as children of `parent`. Stage `lit` is drawn
/// highlighted. Plumes are spawned for the bottom stage's engines, hidden until throttled.
pub fn spawn(commands: &mut Commands, meshes: &mut Assets<Mesh>, kit: &Kit, parent: Entity, v: &Vessel, from_stage: usize, lit: Option<usize>) -> Layout {
    let mut l = layout(v, from_stage);
    let cone = meshes.add(Cone::new(1.0, 1.0));
    for p in l.pieces.drain(..) {
        let mesh = meshes.add(p.mesh);
        commands.spawn((Mesh3d(mesh), MeshMaterial3d(kit.mat(p.mat, lit == Some(p.stage))), p.tf, ChildOf(parent)));
    }
    for nz in l.nozzles.iter().filter(|n| n.stage == from_stage) {
        let len = nz.radius * 9.0;
        commands.spawn((
            Mesh3d(cone.clone()),
            MeshMaterial3d(kit.plume.clone()),
            Transform::from_translation(nz.at).with_scale(Vec3::new(nz.radius * 1.3, len, nz.radius * 1.3)),
            Visibility::Hidden,
            Plume { len, at: nz.at, radius: nz.radius },
            bevy::light::NotShadowCaster,
            ChildOf(parent),
        ));
    }
    l
}

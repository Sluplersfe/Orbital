//! The world outside the window: planets, the Sun, stars, sky colour, and the launch site with
//! its terrain, service tower and buildings.
//!
//! Rendering is camera-relative: the sim works in f64 metres about a body's centre, so every
//! frame each object is placed at `(position - View::origin)` in f32, in the central body's
//! rotating (body-fixed) frame. See `View` in `gfx/mod.rs`.
use super::{tex, View};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::math::{DQuat, DVec3};
use bevy::prelude::*;
use orbital_client::bodies::{self, Body};
use std::f32::consts::{FRAC_PI_2, TAU};

#[derive(Component)]
pub struct Planet(pub &'static str);

/// The cloud layer and atmosphere haze shell follow their planet.
#[derive(Component)]
pub struct Shell(pub &'static str);

#[derive(Component)]
pub struct Sun;

#[derive(Component)]
pub struct SunLight;

#[derive(Component)]
pub struct Stars;

/// Root of the launch site, in pad-local axes: X east (downrange), Y up, Z south.
#[derive(Component)]
pub struct PadRoot;

/// Earth's orbital angle at world time 0, chosen so the pad starts the game in mid-morning.
const EARTH_PHASE: f64 = std::f64::consts::PI + 0.6;

fn phase(id: &str) -> f64 {
    if id == "earth" {
        return EARTH_PHASE;
    }
    id.bytes().fold(7u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64)) as f64 % TAU as f64
}

/// Position of a body relative to the Sun at world time `t` (circular, coplanar orbits).
pub fn helio(b: &Body, t: f64) -> DVec3 {
    let Some(parent) = b.parent.filter(|_| b.sma > 0.0) else { return DVec3::ZERO };
    let parent = bodies::body(parent);
    let n = (parent.mu / b.sma.powi(3)).sqrt();
    let a = phase(b.id) + n * t;
    helio(parent, t) + DVec3::new(a.cos(), a.sin(), 0.0) * b.sma
}

/// Inertial vector to the central body's rotating frame.
pub fn fix(v: DVec3, spin: f64) -> DVec3 {
    DQuat::from_rotation_z(-spin) * v
}

pub fn to_r(v: DVec3) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, v.z as f32)
}

/// Pad-local axes (east, up, south) in the body-fixed frame; the pad sits at (R, 0, 0).
pub fn pad_rotation() -> Quat {
    Quat::from_mat3(&Mat3::from_cols(Vec3::Y, Vec3::X, -Vec3::Z))
}

pub fn pad_point() -> DVec3 {
    DVec3::new(bodies::earth().radius, 0.0, 0.0)
}

fn texture(b: &Body) -> Image {
    match b.id {
        "earth" => tex::earth(),
        "moon" => tex::rock([0.8, 0.8, 0.78], 3),
        "mars" => tex::rock([0.85, 0.45, 0.25], 5),
        "vesta" => tex::rock([0.7, 0.66, 0.6], 9),
        "jupiter" => tex::gas([0.85, 0.75, 0.6], [0.65, 0.45, 0.3], 13),
        "neptune" => tex::gas([0.3, 0.45, 0.9], [0.2, 0.3, 0.7], 17),
        "tycho" => tex::rock([0.55, 0.7, 0.8], 19),
        "ymir" => tex::gas([0.8, 0.85, 0.9], [0.6, 0.7, 0.8], 21),
        _ => tex::rock([0.75, 0.8, 0.85], 25),
    }
}

pub fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut images: ResMut<Assets<Image>>) {
    let hi = meshes.add(Sphere::new(1.0).mesh().uv(384, 192));
    let lo = meshes.add(Sphere::new(1.0).mesh().uv(64, 32));
    for b in bodies::bodies().iter().filter(|b| b.id != "sun") {
        let near = matches!(b.id, "earth" | "moon" | "mars" | "halla" | "tycho");
        let mat = materials.add(StandardMaterial { base_color_texture: Some(images.add(texture(b))), perceptual_roughness: 0.9, ..default() });
        commands.spawn((Planet(b.id), Mesh3d(if near { hi.clone() } else { lo.clone() }), MeshMaterial3d(mat), Transform::default(), NotShadowCaster));
        if b.has_atmosphere() {
            let tint = if b.id == "earth" { Color::srgba(0.35, 0.6, 1.0, 0.22) } else { Color::srgba(0.9, 0.6, 0.4, 0.18) };
            let haze = materials.add(StandardMaterial { base_color: tint, alpha_mode: AlphaMode::Blend, perceptual_roughness: 1.0, reflectance: 0.0, ..default() });
            commands.spawn((Shell(b.id), Mesh3d(hi.clone()), MeshMaterial3d(haze), Transform::default(), NotShadowCaster, NotShadowReceiver));
        }
    }
    let clouds = materials.add(StandardMaterial { base_color_texture: Some(images.add(tex::clouds())), alpha_mode: AlphaMode::Blend, perceptual_roughness: 1.0, reflectance: 0.0, ..default() });
    commands.spawn((Shell("earth_clouds"), Mesh3d(hi.clone()), MeshMaterial3d(clouds), Transform::default(), NotShadowCaster, NotShadowReceiver));

    let sun = materials.add(StandardMaterial { base_color: Color::WHITE, emissive: LinearRgba::rgb(400.0, 360.0, 300.0), unlit: true, ..default() });
    commands.spawn((Sun, Mesh3d(lo.clone()), MeshMaterial3d(sun), Transform::default(), NotShadowCaster));
    commands.spawn((SunLight, DirectionalLight { shadow_maps_enabled: true, ..default() }, Transform::default()));

    let stars = materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, alpha_mode: AlphaMode::Add, ..default() });
    commands.spawn((Stars, Mesh3d(meshes.add(star_mesh())), MeshMaterial3d(stars), Transform::default(), NotShadowCaster, NotShadowReceiver));

    let pad = commands.spawn((PadRoot, Transform::default(), Visibility::default())).id();
    build_site(&mut commands, &mut meshes, &mut materials, pad);
}

/// Stars as small quads on a shell far beyond the planets, each facing the centre.
fn star_mesh() -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let (mut pos, mut col, mut idx) = (vec![], vec![], vec![]);
    let dist = 5.0e12f32;
    for i in 0..2500u32 {
        let u = tex::noise(Vec3::new(i as f32 * 1.37, 0.5, 2.1), 1) * 2.0 - 1.0;
        let a = tex::noise(Vec3::new(i as f32 * 0.73, 7.5, 1.3), 2) * TAU * 3.0;
        let d = Vec3::new((1.0 - u * u).sqrt() * a.cos(), (1.0 - u * u).sqrt() * a.sin(), u).normalize();
        let b = tex::noise(Vec3::new(i as f32 * 2.11, 3.3, 9.9), 3);
        let size = dist * (0.0009 + 0.0022 * b * b);
        let side = d.any_orthonormal_vector();
        let up = d.cross(side);
        let c = d * dist;
        let n = pos.len() as u32;
        for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            pos.push((c + side * sx * size + up * sy * size).to_array());
        }
        let warm = tex::noise(Vec3::new(i as f32 * 0.37, 1.0, 5.0), 4);
        let v = 0.3 + 2.5 * b * b;
        let rgb = [v * (0.8 + 0.2 * warm), v * 0.9, v * (1.0 - 0.25 * warm)];
        for _ in 0..4 {
            col.push([rgb[0], rgb[1], rgb[2], 1.0]);
        }
        idx.extend([n, n + 2, n + 1, n, n + 3, n + 2, n, n + 1, n + 2, n, n + 2, n + 3]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(Indices::U32(idx))
}

/// Ground around the pad: a curved cap out to 400 km, finely tessellated near the pad, so the
/// ground under the rocket is exact even though the planet sphere is coarse.
fn terrain_mesh() -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let r = bodies::earth().radius;
    let segs = 128u32;
    let mut rings = vec![0.0f64];
    let mut d = 25.0f64;
    while d < 400_000.0 {
        rings.push(d);
        d *= 1.15;
    }
    let (mut pos, mut col, mut nor, mut idx) = (vec![], vec![], vec![], vec![]);
    for (ri, &d) in rings.iter().enumerate() {
        let a = d / r;
        let n = if ri == 0 { 1 } else { segs };
        for si in 0..n {
            let phi = si as f64 / segs as f64 * std::f64::consts::TAU;
            let x = r * a.sin() * phi.cos();
            let z = r * a.sin() * phi.sin();
            let y = r * a.cos() - r - 1.5;
            pos.push([x as f32, y as f32, z as f32]);
            let up = DVec3::new(a.sin() * phi.cos(), a.cos(), a.sin() * phi.sin());
            nor.push([up.x as f32, up.y as f32, up.z as f32]);
            let p = Vec3::new(x as f32, 0.0, z as f32);
            let g = tex::fbm(p / 900.0, 4, 31);
            let fine = tex::noise(p / 60.0, 33);
            let mut c = [0.2 + 0.12 * g + 0.04 * fine, 0.3 + 0.12 * g, 0.12 + 0.05 * g];
            if d < 260.0 {
                c = [0.36, 0.35, 0.32];
            }
            let far = ((d - 20_000.0) / 200_000.0).clamp(0.0, 1.0) as f32;
            for k in 0..3 {
                c[k] += ([0.3, 0.38, 0.32][k] - c[k]) * far * 0.6;
            }
            col.push([c[0], c[1], c[2], 1.0]);
        }
    }
    // Centre fan, then quads between rings.
    for si in 0..segs {
        idx.extend([0, 1 + (si + 1) % segs, 1 + si]);
    }
    for ri in 1..rings.len() as u32 - 1 {
        let a = 1 + (ri - 1) * segs;
        let b = a + segs;
        for si in 0..segs {
            let s2 = (si + 1) % segs;
            idx.extend([a + si, a + s2, b + si, a + s2, b + s2, b + si]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(Indices::U32(idx))
}

/// Height of the curved ground at distance `d` from the pad (it drops away with curvature).
fn ground_y(d: f32) -> f32 {
    -d * d / (2.0 * bodies::earth().radius as f32) - 1.5
}

fn build_site(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, pad: Entity) {
    let mut mat = |c: Color, metal: f32, rough: f32| materials.add(StandardMaterial { base_color: c, metallic: metal, perceptual_roughness: rough, ..default() });
    let ground = mat(Color::WHITE, 0.0, 0.95);
    let concrete = mat(Color::srgb(0.62, 0.61, 0.58), 0.0, 0.9);
    let dark = mat(Color::srgb(0.12, 0.12, 0.13), 0.0, 0.8);
    let red = mat(Color::srgb(0.7, 0.12, 0.08), 0.3, 0.6);
    let steel = mat(Color::srgb(0.75, 0.76, 0.78), 0.8, 0.4);
    let white = mat(Color::srgb(0.88, 0.88, 0.86), 0.0, 0.6);
    let yellow = mat(Color::srgb(0.9, 0.7, 0.1), 0.2, 0.6);
    let glass = mat(Color::srgb(0.08, 0.12, 0.16), 0.5, 0.1);
    let tree = mat(Color::srgb(0.08, 0.2, 0.08), 0.0, 0.9);
    let rock = mat(Color::srgb(0.38, 0.36, 0.34), 0.0, 0.95);
    let snow = mat(Color::srgb(0.95, 0.96, 0.98), 0.0, 0.7);

    let mut put = |mesh: Mesh, m: &Handle<StandardMaterial>, tf: Transform| {
        commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(m.clone()), tf, ChildOf(pad)));
    };
    put(terrain_mesh(), &ground, Transform::default());

    // Pad: plinth, flame trench, hold-down pillars and two lightning masts.
    put(Cylinder::new(28.0, 3.0).mesh().resolution(64).build(), &concrete, Transform::from_xyz(0.0, -1.5, 0.0));
    put(Cuboid::new(7.0, 0.1, 34.0).into(), &dark, Transform::from_xyz(0.0, 0.02, 10.0));
    for a in 0..4 {
        let ang = a as f32 * FRAC_PI_2 + 0.785;
        put(Cuboid::new(0.9, 3.0, 0.9).into(), &yellow, Transform::from_xyz(ang.cos() * 4.5, 1.5, ang.sin() * 4.5));
    }
    for (x, z) in [(32.0, 30.0), (-32.0, -30.0)] {
        put(Cylinder::new(0.6, 80.0).into(), &steel, Transform::from_xyz(x, 40.0, z));
    }

    // Service tower: a lattice west of the rocket, with an access arm.
    let (tx, th) = (-11.0f32, 64.0f32);
    for (cx, cz) in [(-2.5f32, -2.5f32), (2.5, -2.5), (-2.5, 2.5), (2.5, 2.5)] {
        put(Cuboid::new(0.5, th, 0.5).into(), &red, Transform::from_xyz(tx + cx, th / 2.0, cz));
    }
    let mut y = 4.0;
    while y < th {
        for (sx, sz, w, d) in [(0.0, -2.5, 5.0, 0.3), (0.0, 2.5, 5.0, 0.3), (-2.5, 0.0, 0.3, 5.0), (2.5, 0.0, 0.3, 5.0)] {
            put(Cuboid::new(w, 0.3, d).into(), &red, Transform::from_xyz(tx + sx, y, sz));
        }
        for (sz, side) in [(-2.5f32, 1.0f32), (2.5, -1.0)] {
            put(Cuboid::new(6.4, 0.2, 0.2).into(), &red, Transform::from_xyz(tx, y - 2.0, sz).with_rotation(Quat::from_rotation_z(side * 0.67)));
        }
        y += 4.0;
    }
    put(Cuboid::new(6.0, 1.0, 6.0).into(), &steel, Transform::from_xyz(tx, th + 0.5, 0.0));
    put(Cuboid::new(7.5, 1.2, 1.6).into(), &steel, Transform::from_xyz(tx + 5.5, 18.0, 0.0));

    // Buildings: assembly hall, control tower, tank farm, radar dome.
    put(Cuboid::new(70.0, 50.0, 55.0).into(), &white, Transform::from_xyz(-170.0, 25.0, -140.0));
    put(Cuboid::new(24.0, 40.0, 0.5).into(), &dark, Transform::from_xyz(-170.0, 20.0, -112.3));
    put(Cylinder::new(4.0, 36.0).into(), &white, Transform::from_xyz(-120.0, 18.0, 95.0));
    put(Cylinder::new(7.0, 6.0).into(), &white, Transform::from_xyz(-120.0, 39.0, 95.0));
    put(Cylinder::new(7.05, 3.0).into(), &glass, Transform::from_xyz(-120.0, 39.5, 95.0));
    for (i, (x, z)) in [(70.0f32, -80.0f32), (82.0, -80.0), (94.0, -80.0)].into_iter().enumerate() {
        put(Cylinder::new(5.0, 12.0 + i as f32).into(), &white, Transform::from_xyz(x, 6.0, z));
    }
    for (x, z) in [(90.0f32, 70.0f32), (106.0, 70.0)] {
        put(Sphere::new(6.5).mesh().uv(32, 16), &steel, Transform::from_xyz(x, 7.0, z));
    }
    put(Sphere::new(10.0).mesh().uv(32, 16), &white, Transform::from_xyz(150.0, 2.0, -40.0));
    put(Cuboid::new(400.0, 0.1, 9.0).into(), &dark, Transform::from_xyz(-30.0, -1.45, 160.0));

    // Trees in loose clusters around the site.
    let cone = meshes.add(Cone::new(2.6, 10.0));
    for i in 0..500u32 {
        let a = tex::noise(Vec3::new(i as f32 * 0.91, 1.0, 0.0), 41) * TAU * 4.0;
        let d = 260.0 + tex::noise(Vec3::new(i as f32 * 1.7, 2.0, 0.0), 43) * 1800.0;
        let (x, z) = (a.cos() * d, a.sin() * d);
        if x < -100.0 && x > -260.0 && z < -60.0 && z > -220.0 {
            continue;
        }
        let s = 0.7 + tex::noise(Vec3::new(i as f32, 3.0, 0.0), 45) * 0.8;
        commands.spawn((Mesh3d(cone.clone()), MeshMaterial3d(tree.clone()), Transform::from_xyz(x, ground_y(d) + 5.0 * s, z).with_scale(Vec3::splat(s)), ChildOf(pad)));
    }

    // Mountains to the north and west, with snow caps.
    for i in 0..14u32 {
        let a = std::f32::consts::PI * (0.55 + 0.9 * tex::noise(Vec3::new(i as f32 * 1.3, 5.0, 0.0), 51));
        let d = 18_000.0 + 30_000.0 * tex::noise(Vec3::new(i as f32 * 2.3, 6.0, 0.0), 53);
        let h = 1_400.0 + 1_400.0 * tex::noise(Vec3::new(i as f32 * 3.1, 7.0, 0.0), 55);
        let rad = h * 2.4;
        let (x, z) = (a.cos() * d, -a.sin() * d);
        let base = ground_y(d) - 60.0;
        commands.spawn((Mesh3d(meshes.add(Cone::new(rad, h).mesh().resolution(24).build())), MeshMaterial3d(rock.clone()), Transform::from_xyz(x, base + h / 2.0, z), ChildOf(pad)));
        let sh = h * 0.28;
        commands.spawn((Mesh3d(meshes.add(Cone::new(rad * 0.28 * 1.02, sh).mesh().resolution(24).build())), MeshMaterial3d(snow.clone()), Transform::from_xyz(x, base + h - sh / 2.0 + 2.0, z), ChildOf(pad)));
    }
}

/// Place planets, Sun, light and stars for this frame; set the sky colour and ambient light.
pub fn place(
    view: Res<View>,
    mut planets: Query<(&Planet, &mut Transform, &mut Visibility), (Without<Shell>, Without<Sun>, Without<SunLight>, Without<Stars>, Without<PadRoot>)>,
    mut shells: Query<(&Shell, &mut Transform, &mut Visibility), (Without<Planet>, Without<Sun>, Without<SunLight>, Without<Stars>, Without<PadRoot>)>,
    mut sun: Query<&mut Transform, (With<Sun>, Without<Planet>, Without<Shell>, Without<SunLight>, Without<Stars>, Without<PadRoot>)>,
    mut light: Query<(&mut Transform, &mut DirectionalLight), (With<SunLight>, Without<Planet>, Without<Shell>, Without<Sun>, Without<Stars>, Without<PadRoot>)>,
    mut stars: Query<(&mut Transform, &MeshMaterial3d<StandardMaterial>), (With<Stars>, Without<Planet>, Without<Shell>, Without<Sun>, Without<SunLight>, Without<PadRoot>)>,
    mut pad: Query<(&mut Transform, &mut Visibility), (With<PadRoot>, Without<Planet>, Without<Shell>, Without<Sun>, Without<SunLight>, Without<Stars>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
) {
    let t = view.time;
    let centre = helio(view.body, t);
    let rel = |b: &Body| fix(helio(b, t) - centre, view.spin) - view.origin;
    let spin_of = |b: &Body| DQuat::from_rotation_z(b.omega * t - if b.id == view.body.id { b.omega * t } else { view.spin });
    // Bevy's UV sphere already has its pole on +Z, like the sim.
    let base = Quat::IDENTITY;
    for (p, mut tf, mut vis) in &mut planets {
        let b = bodies::body(p.0);
        let r = rel(b);
        // The central body is drawn a little low so its coarse facets never poke through the
        // finely tessellated ground around the pad.
        let radius = if b.id == view.body.id { b.radius - 400.0 } else { b.radius };
        *tf = Transform::from_translation(to_r(r)).with_rotation(dq(spin_of(b)) * base).with_scale(Vec3::splat(radius as f32));
        *vis = if r.length() < 5.0e12 { Visibility::Visible } else { Visibility::Hidden };
    }
    for (s, mut tf, mut vis) in &mut shells {
        let id = s.0.trim_end_matches("_clouds");
        let b = bodies::body(id);
        let r = rel(b);
        let radius = if s.0.ends_with("_clouds") { b.radius + 5_000.0 } else { b.radius + b.atmo_top * 0.6 };
        *tf = Transform::from_translation(to_r(r)).with_rotation(dq(spin_of(b)) * base).with_scale(Vec3::splat(radius as f32));
        // Seen from inside, the shells would tint everything: hide them below their radius.
        let inside = (view.origin - fix(helio(b, t) - centre, view.spin)).length() < radius;
        *vis = if inside { Visibility::Hidden } else { Visibility::Visible };
    }
    let sun_rel = rel(bodies::body("sun"));
    let sun_dir = sun_rel.normalize();
    if let Ok(mut tf) = sun.single_mut() {
        *tf = Transform::from_translation(to_r(sun_rel)).with_scale(Vec3::splat(6.957e8));
    }
    let sd = to_r(sun_dir);
    if let Ok((mut tf, mut l)) = light.single_mut() {
        *tf = Transform::default().looking_to(-sd, if sd.z.abs() > 0.99 { Vec3::X } else { Vec3::Z });
        // Night side: the planet is in the way of the Sun.
        let o = view.origin;
        let tca = -o.dot(sun_dir);
        let closest = (o + sun_dir * tca.max(0.0)).length();
        let shadowed = tca > 0.0 && closest < view.body.radius;
        l.illuminance = if shadowed { 0.0 } else { 10_000.0 };
    }
    if let Ok((mut tf, m)) = stars.single_mut() {
        tf.rotation = dq(DQuat::from_rotation_z(-view.spin));
        // Stars fade in as the sky darkens.
        let dark = sky_dark(&view, sun_dir);
        if let Some(mut mat) = materials.get_mut(&m.0) {
            mat.base_color = Color::srgba(1.0, 1.0, 1.0, dark);
        }
    }
    let up = view.origin.normalize_or_zero();
    let elev = up.dot(sun_dir) as f32;
    let air = air(&view);
    let day = ((elev + 0.1) * 4.0).clamp(0.0, 1.0);
    let k = air * day;
    let sky = Vec3::new(0.32, 0.55, 0.9) * k + Vec3::new(0.9, 0.45, 0.2) * air * (1.0 - (elev.abs() * 6.0).min(1.0)) * 0.35;
    clear.0 = Color::linear_rgb(sky.x * 0.6, sky.y * 0.6, sky.z * 0.6);
    ambient.brightness = 40.0 + 500.0 * k;
    ambient.color = Color::srgb(0.7 + 0.3 * (1.0 - air), 0.8 + 0.2 * (1.0 - air), 1.0);

    if let Ok((mut tf, mut vis)) = pad.single_mut() {
        let r = pad_point() - view.origin;
        tf.translation = to_r(r);
        tf.rotation = pad_rotation();
        // From high up the site's ground disc would stand out against the planet texture.
        *vis = if view.body.id == "earth" && r.length() < 60_000.0 { Visibility::Visible } else { Visibility::Hidden };
    }
}

/// How much air is overhead: 1 on the ground, 0 above the atmosphere.
fn air(view: &View) -> f32 {
    let b = view.body;
    if !b.has_atmosphere() {
        return 0.0;
    }
    let alt = view.origin.length() - b.radius;
    ((-alt / (b.scale_h * 1.6)).exp() as f32).clamp(0.0, 1.0) * (b.rho0 / 1.225).min(1.0) as f32
}

fn sky_dark(view: &View, sun_dir: DVec3) -> f32 {
    let up = view.origin.normalize_or_zero();
    let elev = up.dot(sun_dir) as f32;
    let day = ((elev + 0.1) * 4.0).clamp(0.0, 1.0);
    1.0 - air(view) * day
}

pub fn dq(q: DQuat) -> Quat {
    Quat::from_xyzw(q.x as f32, q.y as f32, q.z as f32, q.w as f32)
}

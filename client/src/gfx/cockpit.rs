//! The pilot's seat: a cabin around the camera with a window, a dashboard of analog gauges, a
//! navball and warning lamps, and controls you operate with the mouse: drag the throttle lever
//! and the stick, lift the cover off the staging button and press it, flip the parachute switch.
//! Also the station cabin used by mission control in orbit.
//!
//! Seat-local axes: the eye is at the origin, looking down -Z, with +Y toward the pilot's head.
use super::{tex, Shot, View};
use crate::{Game, Mode};
use bevy::picking::events::{PointerClick, PointerDrag};
use bevy::picking::pointer::PointerButton;
use bevy::picking::Pickable;
use bevy::prelude::*;
use orbital_client::parts::Kind;
use orbital_client::program::Loc;
use std::f32::consts::PI;

/// Tilt of the dashboard: its top leans away from the pilot.
const DASH_TILT: f32 = -0.5;

#[derive(Resource, Default)]
pub struct Controls {
    pub cover_open: bool,
    /// Stick deflection shown on the model, -1..1 (decays back to centre).
    pub stick: f32,
}

#[derive(Component)]
pub struct CockpitRoot;

#[derive(Component)]
pub struct CabinRoot;

#[derive(Component)]
pub struct Navball;

#[derive(Component)]
pub struct Lever;

#[derive(Component)]
pub struct Cover;

#[derive(Component)]
pub struct ChuteSwitch;

#[derive(Component)]
pub struct Stick;

#[derive(Component, Clone, Copy)]
pub enum Gauge {
    AltFine,
    AltCoarse,
    Speed,
    Fuel,
    Pressure,
}

#[derive(Component, Clone, Copy)]
pub enum Lamp {
    MaxQ,
    Fuel,
    Chute,
    Space,
}

/// A crew member sitting in the station cabin; shown when that seat is taken.
#[derive(Component)]
pub struct Figure(usize);

fn put(c: &mut Commands, meshes: &mut Assets<Mesh>, mesh: Mesh, m: &Handle<StandardMaterial>, tf: Transform, parent: Entity) -> Entity {
    c.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(m.clone()), tf, ChildOf(parent))).id()
}

fn dash_rot() -> Quat {
    Quat::from_rotation_x(DASH_TILT)
}

pub fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut images: ResMut<Assets<Image>>) {
    let mut mat = |c: Color, metal: f32, rough: f32| materials.add(StandardMaterial { base_color: c, metallic: metal, perceptual_roughness: rough, ..default() });
    let shell = mat(Color::srgb(0.22, 0.23, 0.25), 0.3, 0.7);
    let panel = mat(Color::srgb(0.09, 0.095, 0.1), 0.2, 0.6);
    let frame = mat(Color::srgb(0.32, 0.33, 0.35), 0.7, 0.4);
    let black = mat(Color::srgb(0.02, 0.02, 0.02), 0.0, 0.5);
    let cushion = mat(Color::srgb(0.12, 0.16, 0.24), 0.0, 0.9);
    let hazard = mat(Color::srgb(0.85, 0.65, 0.05), 0.0, 0.6);
    let red = materials.add(StandardMaterial { base_color: Color::srgb(0.8, 0.05, 0.03), emissive: LinearRgba::rgb(0.6, 0.02, 0.01), ..default() });
    let cover = materials.add(StandardMaterial { base_color: Color::srgba(0.9, 0.15, 0.1, 0.45), alpha_mode: AlphaMode::Blend, ..default() });
    let orange = materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.5, 0.1), emissive: LinearRgba::rgb(0.8, 0.3, 0.05), ..default() });
    let dial_img = images.add(tex::dial());
    let dial = materials.add(StandardMaterial { base_color_texture: Some(dial_img.clone()), emissive_texture: Some(dial_img), emissive: LinearRgba::rgb(0.6, 0.6, 0.55), ..default() });
    let screen_img = images.add(tex::screen([0.3, 1.0, 0.6]));
    let screen = materials.add(StandardMaterial { base_color_texture: Some(screen_img.clone()), emissive_texture: Some(screen_img), emissive: LinearRgba::rgb(2.0, 2.0, 2.0), ..default() });
    let ball_img = images.add(tex::navball());
    let ball = materials.add(StandardMaterial { base_color_texture: Some(ball_img.clone()), emissive_texture: Some(ball_img), emissive: LinearRgba::rgb(0.5, 0.5, 0.5), perceptual_roughness: 0.3, ..default() });
    let needle_mat = |c: LinearRgba| StandardMaterial { base_color: Color::WHITE, emissive: c, ..default() };

    let root = commands.spawn((CockpitRoot, Transform::default(), Visibility::Hidden)).id();

    // Shell: floor, roof, back wall, side walls with side windows, window pillars and frame.
    put(&mut commands, &mut meshes, Cuboid::new(2.0, 0.06, 1.9).into(), &shell, Transform::from_xyz(0.0, -1.05, -0.1), root);
    put(&mut commands, &mut meshes, Cuboid::new(2.0, 0.06, 1.9).into(), &shell, Transform::from_xyz(0.0, 0.88, -0.1), root);
    put(&mut commands, &mut meshes, Cuboid::new(2.0, 2.0, 0.06).into(), &shell, Transform::from_xyz(0.0, -0.1, 0.85), root);
    for side in [-1.0f32, 1.0] {
        put(&mut commands, &mut meshes, Cuboid::new(0.06, 1.0, 1.9).into(), &shell, Transform::from_xyz(side * 1.0, -0.55, -0.1), root);
        put(&mut commands, &mut meshes, Cuboid::new(0.06, 0.25, 1.9).into(), &shell, Transform::from_xyz(side * 1.0, 0.76, -0.1), root);
        put(&mut commands, &mut meshes, Cuboid::new(0.06, 0.7, 0.2).into(), &shell, Transform::from_xyz(side * 1.0, 0.3, 0.6), root);
        put(&mut commands, &mut meshes, Cuboid::new(0.12, 1.15, 0.12).into(), &frame, Transform::from_xyz(side * 0.8, 0.3, -0.88).with_rotation(Quat::from_rotation_z(side * 0.22)), root);
        put(&mut commands, &mut meshes, Cuboid::new(0.1, 0.1, 1.2).into(), &frame, Transform::from_xyz(side * 0.97, 0.0, -0.3), root);
    }
    put(&mut commands, &mut meshes, Cuboid::new(1.9, 0.14, 0.22).into(), &frame, Transform::from_xyz(0.0, 0.82, -0.82), root);
    put(&mut commands, &mut meshes, Cuboid::new(0.04, 0.9, 0.04).into(), &frame, Transform::from_xyz(0.0, 0.36, -0.98), root);

    // Seat behind and below the eye.
    put(&mut commands, &mut meshes, Cuboid::new(0.6, 0.85, 0.12).into(), &cushion, Transform::from_xyz(0.0, -0.3, 0.4), root);
    put(&mut commands, &mut meshes, Cuboid::new(0.6, 0.12, 0.6).into(), &cushion, Transform::from_xyz(0.0, -0.8, 0.05), root);

    // Glare shield and dashboard.
    put(&mut commands, &mut meshes, Cuboid::new(1.7, 0.05, 0.32).into(), &panel, Transform::from_xyz(0.0, -0.1, -0.86), root);
    let dash = put(&mut commands, &mut meshes, Cuboid::new(1.6, 0.58, 0.05).into(), &panel, Transform::from_xyz(0.0, -0.38, -0.7).with_rotation(dash_rot()), root);
    let face = 0.03;

    // Navball in a bezel, with a fixed aircraft marker.
    commands.spawn((Navball, Mesh3d(meshes.add(Sphere::new(0.1).mesh().uv(48, 24))), MeshMaterial3d(ball), Transform::from_xyz(0.0, 0.02, face + 0.02), ChildOf(dash)));
    put(&mut commands, &mut meshes, Torus::new(0.098, 0.118).into(), &frame, Transform::from_xyz(0.0, 0.02, face + 0.03).with_rotation(Quat::from_rotation_x(PI / 2.0)), dash);
    put(&mut commands, &mut meshes, Cuboid::new(0.07, 0.008, 0.006).into(), &orange, Transform::from_xyz(0.0, 0.02, face + 0.125), dash);
    put(&mut commands, &mut meshes, Cuboid::new(0.008, 0.03, 0.006).into(), &orange, Transform::from_xyz(0.0, 0.007, face + 0.125), dash);

    // Gauges: dial faces with needles, each needle a different colour.
    let gauges = [
        (Gauge::AltFine, -0.62, 0.12, LinearRgba::rgb(3.0, 3.0, 3.0), 0.065),
        (Gauge::AltCoarse, -0.62, 0.12, LinearRgba::rgb(3.0, 1.2, 0.2), 0.04),
        (Gauge::Speed, -0.43, 0.12, LinearRgba::rgb(0.3, 2.5, 3.0), 0.065),
        (Gauge::Fuel, 0.43, 0.12, LinearRgba::rgb(3.0, 2.0, 0.2), 0.065),
        (Gauge::Pressure, 0.62, 0.12, LinearRgba::rgb(3.0, 0.3, 0.2), 0.065),
    ];
    for (g, x, y, c, len) in gauges {
        if !matches!(g, Gauge::AltCoarse) {
            put(&mut commands, &mut meshes, Circle::new(0.08).into(), &dial, Transform::from_xyz(x, y, face), dash);
            put(&mut commands, &mut meshes, Torus::new(0.08, 0.09).into(), &frame, Transform::from_xyz(x, y, face).with_rotation(Quat::from_rotation_x(PI / 2.0)), dash);
        }
        let pivot = commands.spawn((g, Transform::from_xyz(x, y, face + 0.004 + len * 0.01), Visibility::default(), ChildOf(dash))).id();
        let m = materials.add(needle_mat(c));
        commands.spawn((Mesh3d(meshes.add(Cuboid::new(0.006, len, 0.003))), MeshMaterial3d(m), Transform::from_xyz(0.0, len / 2.0 - 0.008, 0.0), ChildOf(pivot)));
    }

    // Two screens either side of the navball.
    for x in [-0.22f32, 0.22] {
        put(&mut commands, &mut meshes, Rectangle::new(0.2, 0.14).into(), &screen, Transform::from_xyz(x, 0.04, face), dash);
    }

    // Warning lamps along the bottom of the panel.
    for (i, l) in [Lamp::MaxQ, Lamp::Fuel, Lamp::Chute, Lamp::Space].into_iter().enumerate() {
        let m = materials.add(StandardMaterial { base_color: Color::srgb(0.1, 0.1, 0.1), ..default() });
        commands.spawn((l, Mesh3d(meshes.add(Cuboid::new(0.06, 0.03, 0.012))), MeshMaterial3d(m), Transform::from_xyz(-0.15 + i as f32 * 0.1, -0.2, face), ChildOf(dash)));
    }

    // Side consoles.
    for side in [-1.0f32, 1.0] {
        put(&mut commands, &mut meshes, Cuboid::new(0.34, 0.5, 0.9).into(), &panel, Transform::from_xyz(side * 0.64, -0.8, -0.3), root);
    }

    // Throttle lever on the left console: drag it forward (up the screen) for more thrust.
    put(&mut commands, &mut meshes, Cuboid::new(0.05, 0.01, 0.26).into(), &black, Transform::from_xyz(-0.62, -0.545, -0.35), root);
    let lever = commands.spawn((Lever, Transform::from_xyz(-0.62, -0.55, -0.35), Visibility::default(), ChildOf(root))).id();
    put(&mut commands, &mut meshes, Cylinder::new(0.012, 0.22).into(), &frame, Transform::from_xyz(0.0, 0.11, 0.0), lever);
    commands
        .spawn((Mesh3d(meshes.add(Cuboid::new(0.1, 0.05, 0.07))), MeshMaterial3d(black.clone()), Transform::from_xyz(0.0, 0.23, 0.0), Pickable::default(), ChildOf(lever)))
        .observe(drag_throttle);

    // Staging button under a hinged cover on the right console.
    put(&mut commands, &mut meshes, Cuboid::new(0.14, 0.02, 0.14).into(), &hazard, Transform::from_xyz(0.6, -0.545, -0.45), root);
    commands
        .spawn((Mesh3d(meshes.add(Cylinder::new(0.035, 0.03))), MeshMaterial3d(red), Transform::from_xyz(0.6, -0.52, -0.45), Pickable::default(), ChildOf(root)))
        .observe(press_stage);
    let hinge = commands.spawn((Cover, Transform::from_xyz(0.6, -0.535, -0.51), Visibility::default(), ChildOf(root))).id();
    commands
        .spawn((Mesh3d(meshes.add(Cuboid::new(0.11, 0.06, 0.11))), MeshMaterial3d(cover), Transform::from_xyz(0.0, 0.03, 0.06), Pickable::default(), ChildOf(hinge)))
        .observe(flip_cover);

    // Parachute switch.
    put(&mut commands, &mut meshes, Cuboid::new(0.08, 0.02, 0.06).into(), &black, Transform::from_xyz(0.6, -0.545, -0.2), root);
    let sw = commands.spawn((ChuteSwitch, Transform::from_xyz(0.6, -0.535, -0.2), Visibility::default(), ChildOf(root))).id();
    commands
        .spawn((Mesh3d(meshes.add(Cylinder::new(0.012, 0.08))), MeshMaterial3d(orange.clone()), Transform::from_xyz(0.0, 0.04, 0.0), Pickable::default(), ChildOf(sw)))
        .observe(flip_chute);

    // Control stick between the knees: drag left or right to pitch.
    let stick = commands.spawn((Stick, Transform::from_xyz(0.0, -0.95, -0.32), Visibility::default(), ChildOf(root))).id();
    put(&mut commands, &mut meshes, Cylinder::new(0.015, 0.36).into(), &frame, Transform::from_xyz(0.0, 0.18, 0.0), stick);
    commands
        .spawn((Mesh3d(meshes.add(Capsule3d::new(0.032, 0.09))), MeshMaterial3d(black.clone()), Transform::from_xyz(0.0, 0.4, 0.0), Pickable::default(), ChildOf(stick)))
        .observe(drag_stick);

    commands.spawn((PointLight { intensity: 40_000.0, range: 4.0, color: Color::srgb(1.0, 0.92, 0.8), ..default() }, Transform::from_xyz(0.0, 0.7, 0.2), ChildOf(root)));

    cabin(&mut commands, &mut meshes, &shell, &frame, &cushion, &panel, &mut materials);
}

/// Station cabin: a room with a big window toward the planet, benches and the crew.
fn cabin(commands: &mut Commands, meshes: &mut Assets<Mesh>, shell: &Handle<StandardMaterial>, frame: &Handle<StandardMaterial>, cushion: &Handle<StandardMaterial>, panel: &Handle<StandardMaterial>, materials: &mut Assets<StandardMaterial>) {
    let root = commands.spawn((CabinRoot, Transform::default(), Visibility::Hidden)).id();
    let mut put = |c: &mut Commands, mesh: Mesh, m: &Handle<StandardMaterial>, tf: Transform| {
        put(c, meshes, mesh, m, tf, root);
    };
    // Window wall at z = -2.2 with a 3.2 x 1.6 opening.
    put(commands, Cuboid::new(5.0, 1.2, 0.1).into(), shell, Transform::from_xyz(0.0, -1.3, -2.2));
    put(commands, Cuboid::new(5.0, 0.8, 0.1).into(), shell, Transform::from_xyz(0.0, 1.5, -2.2));
    for side in [-1.0f32, 1.0] {
        put(commands, Cuboid::new(0.9, 1.6, 0.1).into(), shell, Transform::from_xyz(side * 2.05, 0.1, -2.2));
        put(commands, Cuboid::new(0.1, 3.8, 4.6).into(), shell, Transform::from_xyz(side * 2.5, 0.0, 0.0));
        put(commands, Cuboid::new(0.12, 1.7, 0.16).into(), frame, Transform::from_xyz(side * 1.6, 0.1, -2.15));
        // Bench along each wall.
        put(commands, Cuboid::new(0.6, 0.12, 3.0).into(), cushion, Transform::from_xyz(side * 1.9, -1.15, -0.2));
    }
    put(commands, Cuboid::new(3.3, 0.12, 0.16).into(), frame, Transform::from_xyz(0.0, 0.92, -2.15));
    put(commands, Cuboid::new(3.3, 0.12, 0.16).into(), frame, Transform::from_xyz(0.0, -0.72, -2.15));
    put(commands, Cuboid::new(5.0, 0.1, 4.6).into(), shell, Transform::from_xyz(0.0, -1.9, 0.0));
    put(commands, Cuboid::new(5.0, 0.1, 4.6).into(), shell, Transform::from_xyz(0.0, 1.9, 0.0));
    put(commands, Cuboid::new(5.0, 3.8, 0.1).into(), shell, Transform::from_xyz(0.0, 0.0, 2.3));
    // Table under the window.
    put(commands, Cuboid::new(1.4, 0.06, 0.6).into(), panel, Transform::from_xyz(0.6, -0.9, -1.4));

    let suit = materials.add(StandardMaterial { base_color: Color::srgb(0.12, 0.2, 0.42), perceptual_roughness: 0.8, ..default() });
    let skin = materials.add(StandardMaterial { base_color: Color::srgb(0.8, 0.62, 0.5), perceptual_roughness: 0.7, ..default() });
    let seats = [(-1.9f32, -0.9f32), (1.9, -0.6), (1.9, 0.6), (-1.9, 0.5), (-1.9, 1.5), (1.9, 1.6)];
    for (i, (x, z)) in seats.into_iter().enumerate() {
        let f = commands.spawn((Figure(i), Transform::from_xyz(x, -0.65, z), Visibility::Hidden, ChildOf(root))).id();
        commands.spawn((Mesh3d(meshes.add(Capsule3d::new(0.22, 0.55))), MeshMaterial3d(suit.clone()), Transform::default(), ChildOf(f)));
        commands.spawn((Mesh3d(meshes.add(Sphere::new(0.14))), MeshMaterial3d(skin.clone()), Transform::from_xyz(0.0, 0.62, 0.0), ChildOf(f)));
    }
    commands.spawn((PointLight { intensity: 120_000.0, range: 8.0, color: Color::srgb(1.0, 0.95, 0.88), ..default() }, Transform::from_xyz(0.0, 1.6, 0.0), ChildOf(root)));
}

fn drag_throttle(ev: On<PointerDrag>, mut g: ResMut<Game>) {
    if ev.button != PointerButton::Primary {
        return;
    }
    if let Mode::Flying(f) = &mut g.mode {
        let t = f.throttle - ev.delta.y as f64 * 0.006;
        f.set_throttle(t);
    }
}

fn drag_stick(ev: On<PointerDrag>, mut g: ResMut<Game>, mut c: ResMut<Controls>) {
    if ev.button != PointerButton::Primary {
        return;
    }
    if let Mode::Flying(f) = &mut g.mode {
        let p = f.pitch_cmd + ev.delta.x as f64 * 0.15;
        f.set_pitch(p);
        c.stick = (c.stick + ev.delta.x * 0.05).clamp(-1.0, 1.0);
    }
}

fn flip_cover(ev: On<PointerClick>, mut c: ResMut<Controls>) {
    if ev.button == PointerButton::Primary {
        c.cover_open = !c.cover_open;
    }
}

fn press_stage(ev: On<PointerClick>, mut g: ResMut<Game>, c: Res<Controls>) {
    if ev.button != PointerButton::Primary || !c.cover_open {
        return;
    }
    if let Mode::Flying(f) = &mut g.mode {
        f.stage();
    }
}

fn flip_chute(ev: On<PointerClick>, mut g: ResMut<Game>) {
    if ev.button != PointerButton::Primary {
        return;
    }
    if let Mode::Flying(f) = &mut g.mode {
        f.deploy_chute();
    }
}

/// Fraction of the needle sweep to a rotation (sweep runs -135..135 degrees, clockwise).
fn sweep(v: f32) -> Quat {
    Quat::from_rotation_z(-(-135.0 + 270.0 * v.clamp(0.0, 1.0)).to_radians())
}

#[allow(clippy::type_complexity)]
pub fn animate(
    g: Res<Game>,
    view: Res<View>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut controls: ResMut<Controls>,
    mut roots: Query<(&mut Transform, &mut Visibility, Has<CockpitRoot>), Or<(With<CockpitRoot>, With<CabinRoot>)>>,
    mut parts: ParamSet<(
        Query<&mut Transform, With<Navball>>,
        Query<&mut Transform, With<Lever>>,
        Query<&mut Transform, With<Cover>>,
        Query<&mut Transform, With<ChuteSwitch>>,
        Query<&mut Transform, With<Stick>>,
        Query<(&Gauge, &mut Transform)>,
    )>,
    lamps: Query<(&Lamp, &MeshMaterial3d<StandardMaterial>)>,
    mut figures: Query<(&Figure, &mut Visibility), (Without<CockpitRoot>, Without<CabinRoot>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (mut tf, mut vis, is_cockpit) in &mut roots {
        let shown = if is_cockpit { view.shot == Shot::Cockpit } else { view.shot == Shot::Cabin };
        *vis = if shown { Visibility::Visible } else { Visibility::Hidden };
        *tf = Transform::from_rotation(view.seat_rot);
    }
    if view.shot == Shot::Cabin {
        let crew = match g.program.locate(&g.me) {
            Loc::Station(id) => g.program.station(id).map(|s| s.crew.len()).unwrap_or(0),
            Loc::Ship(id) => g.program.ship(id).map(|s| s.crew.len()).unwrap_or(0),
            Loc::Earth => 0,
        };
        // Everyone aboard except you.
        for (f, mut vis) in &mut figures {
            *vis = if f.0 + 1 < crew { Visibility::Visible } else { Visibility::Hidden };
        }
    }
    let Mode::Flying(f) = &g.mode else { return };

    // Navball: show the local horizon (up, east) in the seat frame, mirrored so the point under
    // the marker is where the nose points.
    let base = f.pos.normalize();
    let up = Vec3::new(base.x as f32, base.y as f32, base.z as f32);
    let up = Quat::from_rotation_z(-(view.spin as f32)) * up;
    let east = Vec3::Z.cross(up).normalize();
    // The ball's texture has its sky pole on local +Z (Bevy's UV sphere).
    let horizon = Quat::from_mat3(&Mat3::from_cols(east, Vec3::Z, up));
    let mirror = view.seat_rot * Quat::from_rotation_y(PI);
    if let Ok(mut tf) = parts.p0().single_mut() {
        tf.rotation = dash_rot().inverse() * mirror.inverse() * horizon;
    }
    if let Ok(mut tf) = parts.p1().single_mut() {
        tf.rotation = Quat::from_rotation_x(0.6 - 1.2 * f.throttle as f32);
    }
    if let Ok(mut tf) = parts.p2().single_mut() {
        let target = if controls.cover_open { -2.0 } else { 0.0 };
        let cur = tf.rotation.to_euler(EulerRot::XYZ).0;
        tf.rotation = Quat::from_rotation_x(cur + (target - cur) * (time.delta_secs() * 10.0).min(1.0));
    }
    if let Ok(mut tf) = parts.p3().single_mut() {
        tf.rotation = Quat::from_rotation_x(if f.chute_deployed { 0.5 } else { -0.5 });
    }
    let held = |k| keys.pressed(k);
    let input = if held(KeyCode::KeyD) || held(KeyCode::ArrowRight) { 1.0 } else if held(KeyCode::KeyA) || held(KeyCode::ArrowLeft) { -1.0 } else { 0.0 };
    controls.stick = if input != 0.0 { input } else { controls.stick * (1.0 - (time.delta_secs() * 6.0).min(1.0)) };
    if let Ok(mut tf) = parts.p4().single_mut() {
        tf.rotation = Quat::from_rotation_z(-0.3 * controls.stick);
    }
    let alt = f.alt() as f32;
    let cap = f.vessel.stage_prop(f.stage).max(1.0) as f32;
    let strength = f.vessel.weakest().map_or(80_000.0, |(_, q)| q) as f32;
    for (gauge, mut tf) in &mut parts.p5() {
        let v = match gauge {
            Gauge::AltFine => (alt.max(0.0) % 10_000.0) / 10_000.0,
            Gauge::AltCoarse => alt / 200_000.0,
            Gauge::Speed => f.vel.length() as f32 / 8_000.0,
            Gauge::Fuel => f.prop as f32 / cap,
            Gauge::Pressure => f.q() as f32 / strength,
        };
        tf.rotation = sweep(v);
    }
    let has_chute = f.vessel.parts(f.stage.min(f.vessel.stages.len().saturating_sub(1))).any(|p| matches!(p.kind, Kind::Parachute)) || f.has_chute();
    let blink = (time.elapsed_secs() * 4.0).sin() > 0.0;
    for (lamp, m) in &lamps {
        let (on, c) = match lamp {
            Lamp::MaxQ => (f.q() as f32 > strength * 0.75 && blink, LinearRgba::rgb(8.0, 0.3, 0.2)),
            Lamp::Fuel => (f.prop as f32 <= cap * 0.1, LinearRgba::rgb(8.0, 4.0, 0.2)),
            Lamp::Chute => (f.chute_deployed && has_chute && !f.chute_torn, LinearRgba::rgb(0.3, 8.0, 0.5)),
            Lamp::Space => (f.reached_space, LinearRgba::rgb(0.3, 1.5, 8.0)),
        };
        if let Some(mut mat) = materials.get_mut(&m.0) {
            mat.emissive = if on { c } else { LinearRgba::BLACK };
        }
    }
}

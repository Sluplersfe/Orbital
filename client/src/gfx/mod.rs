//! First-person 3D view. Everything that draws lives here; the sim and program logic stay in the
//! headless library.
//!
//! Frames: the sim is planar, in f64 metres about a body's centre, with the body spinning about
//! +Z. Rendering uses the central body's rotating (body-fixed) frame with the same axes, shifted
//! so `View::origin` (the pilot's eye, or the pad) sits at the render origin. On the pad the ship
//! is fixed in that frame, so nothing jitters from f32 rounding.
pub mod cockpit;
pub mod rocket;
pub mod tex;
pub mod world;

use crate::{Game, Mode, Online};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::math::DVec3;
use bevy::picking::mesh_picking::{MeshPickingPlugin, MeshPickingSettings};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use orbital_client::bodies::{self, Body};
use orbital_client::program::Loc;
use orbital_client::space::Place;
use world::{fix, to_r};

/// Which camera the player is looking through.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shot {
    /// Orbiting the rocket on the pad (builder, research, report).
    Pad,
    /// Standing at the top of the control tower, looking at the pad (mission control at Earth).
    Tower,
    /// In the pilot's seat.
    Cockpit,
    /// Outside, behind the ship (toggle with V in flight).
    Chase,
    /// Inside a station or a ship in orbit, at the window (mission control in space).
    Cabin,
}

#[derive(Resource)]
pub struct View {
    pub shot: Shot,
    pub body: &'static Body,
    /// The central body's rotation angle (rad) at `time`.
    pub spin: f64,
    pub time: f64,
    /// Body-fixed sim position at the render origin.
    pub origin: DVec3,
    /// Where the ship's base is, body-fixed.
    pub ship_base: DVec3,
    /// Ship model orientation: local +Y along the nose.
    pub ship_rot: Quat,
    /// Seat orientation: local -Z forward, +Y the pilot's head.
    pub seat_rot: Quat,
}

impl Default for View {
    fn default() -> Self {
        Self {
            shot: Shot::Pad, body: bodies::earth(), spin: 0.0, time: 0.0, origin: world::pad_point(), ship_base: world::pad_point(),
            ship_rot: world::pad_rotation(), seat_rot: Quat::IDENTITY,
        }
    }
}

/// Mouse-look state. Right mouse drag looks around; the wheel zooms the outside cameras.
#[derive(Resource)]
pub struct Look {
    pub yaw: f32,
    pub pitch: f32,
    pub orbit_yaw: f32,
    pub orbit_pitch: f32,
    pub dist: f32,
    pub chase: bool,
    /// Height of the eye above the ship's base, from the current model.
    pub eye: f32,
    pub height: f32,
}

impl Default for Look {
    fn default() -> Self {
        Self { yaw: 0.0, pitch: 0.0, orbit_yaw: 0.6, orbit_pitch: 0.25, dist: 0.0, chase: false, eye: 2.0, height: 8.0 }
    }
}

#[derive(Component)]
pub struct MainCamera;

/// Parent of the ship model; its children are rebuilt when the design or stage changes.
#[derive(Component, Default)]
pub struct ShipRoot {
    key: String,
}

/// Another player's ship, seen from ours.
#[derive(Component)]
pub struct Other(u64);

pub struct GfxPlugin;

impl Plugin for GfxPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MeshPickingPlugin)
            .insert_resource(MeshPickingSettings { require_markers: true, ..default() })
            .insert_resource(ClearColor(Color::BLACK))
            .init_resource::<View>()
            .init_resource::<Look>()
            .init_resource::<cockpit::Controls>()
            .add_systems(Startup, (setup, world::setup, cockpit::setup))
            .add_systems(
                Update,
                (compute_view, world::place, ship_model, plumes, others, look, camera, cockpit::animate).chain().after(crate::GameSet),
            );
    }
}

fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(rocket::Kit::new(&mut materials));
    commands.spawn((
        MainCamera,
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { fov: 70f32.to_radians(), near: 0.05, ..default() }),
        Bloom::NATURAL,
        bevy::picking::mesh_picking::MeshPickingCamera,
        Transform::default(),
    ));
    commands.spawn((ShipRoot::default(), Transform::default(), Visibility::default()));
}

/// Nose and head directions for a ship at `pos` pitched `pitch_deg` from vertical toward east.
fn attitude(pos: DVec3, pitch_deg: f64) -> (DVec3, DVec3) {
    let up = pos.normalize();
    let east = DVec3::Z.cross(up).normalize();
    let p = pitch_deg.to_radians();
    (up * p.cos() + east * p.sin(), up * p.sin() - east * p.cos())
}

/// Ship model rotation (+Y = nose) and seat rotation (-Z = nose, +Y = head).
fn frames(nose: DVec3, head: DVec3) -> (Quat, Quat) {
    let (n, h) = (to_r(nose).normalize(), to_r(head).normalize());
    let side = Vec3::Z.cross(n).normalize_or(Vec3::X);
    let ship = Quat::from_mat3(&Mat3::from_cols(side, n, side.cross(n)));
    let seat = Quat::from_mat3(&Mat3::from_cols(n.cross(h), h, -n));
    (ship, seat)
}

fn compute_view(g: Res<Game>, look: Res<Look>, mut view: ResMut<View>) {
    let earth = bodies::earth();
    let at_pad = |t: f64, shot: Shot| View { shot, spin: earth.omega * t, time: t, ..View::default() };
    *view = match &g.mode {
        Mode::Flying(f) => {
            let t = g.t0 + f.t;
            let spin = f.body.omega * t;
            let base = fix(f.pos, spin);
            let (nose, head) = attitude(base, f.pitch);
            let (ship_rot, seat_rot) = frames(nose, head);
            let shot = if look.chase { Shot::Chase } else { Shot::Cockpit };
            let along = if look.chase { look.height as f64 * 0.5 } else { look.eye as f64 };
            View { shot, body: f.body, spin, time: t, origin: base + nose * along, ship_base: base, ship_rot, seat_rot }
        }
        Mode::Control => {
            let t = g.program.time;
            let place = match g.program.locate(&g.me) {
                Loc::Earth => None,
                Loc::Ship(id) => g.program.ship(id).map(|s| s.place),
                Loc::Station(id) => g.program.station(id).and_then(|s| s.orbit).map(Place::Orbit),
            };
            match place {
                Some(Place::Orbit(o)) => {
                    let b = o.body();
                    let spin = b.omega * t;
                    let pos = fix(o.position(t), spin);
                    // Window faces the planet; the head points north.
                    let (_, seat) = frames(-pos.normalize(), DVec3::Z);
                    View { shot: Shot::Cabin, body: b, spin, time: t, origin: pos, ship_base: pos, ship_rot: Quat::IDENTITY, seat_rot: seat }
                }
                Some(Place::Landed { body, .. }) => {
                    let b = bodies::body(body);
                    let pos = DVec3::new(b.radius + 3.0, 0.0, 0.0);
                    let (_, seat) = frames(DVec3::Y, DVec3::X);
                    View { shot: Shot::Cabin, body: b, spin: b.omega * t, time: t, origin: pos, ship_base: pos, ship_rot: Quat::IDENTITY, seat_rot: seat }
                }
                _ => at_pad(t, Shot::Tower),
            }
        }
        _ => at_pad(g.program.time, Shot::Pad),
    };
}

/// Rebuild the ship model when the design, stage or builder selection changes, and place it.
fn ship_model(
    mut commands: Commands,
    g: Res<Game>,
    view: Res<View>,
    mut look: ResMut<Look>,
    kit: Res<rocket::Kit>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut root: Query<(Entity, &mut ShipRoot, &mut Transform, &mut Visibility, Option<&Children>)>,
) {
    let Ok((e, mut sr, mut tf, mut vis, children)) = root.single_mut() else { return };
    let (vessel, from, lit) = match &g.mode {
        Mode::Flying(f) => (&f.vessel, f.stage, None),
        Mode::Builder => (&g.vessel, 0, Some(g.stage)),
        _ => (&g.vessel, 0, None),
    };
    let key = format!("{:?}|{from}|{lit:?}", vessel.stages);
    if key != sr.key {
        sr.key = key;
        if let Some(ch) = children {
            for c in ch.iter() {
                commands.entity(c).despawn();
            }
        }
        let l = rocket::spawn(&mut commands, &mut meshes, &kit, e, vessel, from, lit);
        look.eye = l.eye;
        look.height = l.height.max(2.0);
    }
    let shown = !matches!(view.shot, Shot::Cabin | Shot::Cockpit);
    *vis = if shown { Visibility::Visible } else { Visibility::Hidden };
    tf.translation = to_r(view.ship_base - view.origin);
    tf.rotation = view.ship_rot;
}

fn plumes(g: Res<Game>, mut q: Query<(&rocket::Plume, &mut Transform, &mut Visibility)>, time: Res<Time>) {
    let (thr, lit) = match &g.mode {
        Mode::Flying(f) => (f.throttle as f32, f.prop > 0.0 && !f.status.is_over()),
        _ => (0.0, false),
    };
    let flicker = 0.92 + 0.08 * (time.elapsed_secs() * 37.0).sin();
    for (p, mut tf, mut vis) in &mut q {
        let on = lit && thr > 0.01;
        *vis = if on { Visibility::Visible } else { Visibility::Hidden };
        let len = p.len * (0.35 + 0.65 * thr) * flicker;
        tf.translation = p.at - Vec3::Y * (len / 2.0);
        tf.scale = Vec3::new(p.radius * 1.2, len, p.radius * 1.2);
    }
}

/// Other players' ships: a simple white rocket at their reported positions.
fn others(
    mut commands: Commands,
    g: Res<Game>,
    view: Res<View>,
    online: Option<Res<Online>>,
    kit: Res<rocket::Kit>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut q: Query<(Entity, &Other, &mut Transform)>,
) {
    let ships = match (&online, view.body.id) {
        (Some(o), "earth") => o.world.others(),
        _ => vec![],
    };
    let _ = &g;
    for (e, o, mut tf) in &mut q {
        match ships.iter().find(|(id, _)| *id == o.0) {
            Some((_, pos)) => {
                let p = fix(*pos, view.spin);
                tf.translation = to_r(p - view.origin);
                tf.rotation = Quat::from_rotation_arc(Vec3::Y, to_r(p.normalize()));
            }
            None => commands.entity(e).despawn(),
        }
    }
    for (id, _) in ships {
        if q.iter().any(|(_, o, _)| o.0 == id) {
            continue;
        }
        let root = commands.spawn((Other(id), Transform::default(), Visibility::default())).id();
        commands.spawn((Mesh3d(meshes.add(Cylinder::new(1.2, 14.0))), MeshMaterial3d(kit.mat(rocket::Mat::White, false)), Transform::from_xyz(0.0, 7.0, 0.0), ChildOf(root)));
        commands.spawn((Mesh3d(meshes.add(Cone::new(1.2, 3.0))), MeshMaterial3d(kit.mat(rocket::Mat::White, false)), Transform::from_xyz(0.0, 15.5, 0.0), ChildOf(root)));
        commands.spawn((PointLight { color: Color::srgb(0.3, 1.0, 0.4), intensity: 2.0e6, range: 400.0, ..default() }, Transform::from_xyz(0.0, 18.0, 0.0), ChildOf(root)));
    }
}

/// Right mouse drag looks around (or orbits the outside cameras); the wheel zooms.
fn look(buttons: Res<ButtonInput<MouseButton>>, motion: Res<AccumulatedMouseMotion>, scroll: Res<AccumulatedMouseScroll>, keys: Res<ButtonInput<KeyCode>>, g: Res<Game>, view: Res<View>, mut look: ResMut<Look>) {
    if matches!(g.mode, Mode::Flying(_)) && keys.just_pressed(KeyCode::KeyV) {
        look.chase = !look.chase;
    }
    let d = motion.delta * 0.004;
    let outside = matches!(view.shot, Shot::Pad | Shot::Chase);
    if buttons.pressed(MouseButton::Right) {
        if outside {
            look.orbit_yaw -= d.x;
            look.orbit_pitch = (look.orbit_pitch + d.y).clamp(-0.2, 1.45);
        } else {
            look.yaw = (look.yaw - d.x).clamp(-2.2, 2.2);
            look.pitch = (look.pitch - d.y).clamp(-1.3, 1.3);
        }
    }
    if keys.just_pressed(KeyCode::Home) {
        look.yaw = 0.0;
        look.pitch = 0.0;
    }
    if outside && scroll.delta.y != 0.0 {
        let base = look.height * 1.6 + 12.0;
        let cur = if look.dist == 0.0 { base } else { look.dist };
        look.dist = (cur * (1.0 - scroll.delta.y * 0.1)).clamp(look.height * 0.6 + 4.0, 4_000.0);
    }
    if matches!(view.shot, Shot::Pad) && !buttons.pressed(MouseButton::Right) && !matches!(g.mode, Mode::Builder) {
        // Research and the report: drift slowly around the rocket.
        look.orbit_yaw += 0.0015;
    }
}

fn camera(view: Res<View>, look: Res<Look>, mut cam: Query<&mut Transform, With<MainCamera>>) {
    let Ok(mut tf) = cam.single_mut() else { return };
    let head = Quat::from_rotation_y(look.yaw) * Quat::from_rotation_x(look.pitch);
    let dist = if look.dist == 0.0 { look.height * 1.6 + 12.0 } else { look.dist };
    let orbit = Vec3::new(look.orbit_yaw.sin() * look.orbit_pitch.cos(), look.orbit_pitch.sin(), look.orbit_yaw.cos() * look.orbit_pitch.cos()) * dist;
    *tf = match view.shot {
        Shot::Cockpit | Shot::Cabin => Transform::from_rotation(view.seat_rot * head),
        Shot::Pad => {
            let pad = to_r(world::pad_point() - view.origin);
            let rot = world::pad_rotation();
            let target = pad + rot * Vec3::new(0.0, look.height * 0.45, 0.0);
            let eye = target + rot * orbit;
            Transform::from_translation(eye).looking_at(target, rot * Vec3::Y)
        }
        Shot::Tower => {
            let rot = world::pad_rotation();
            let pad = to_r(world::pad_point() - view.origin);
            let eye = pad + rot * Vec3::new(-112.0, 41.5, 88.0);
            let target = pad + rot * Vec3::new(0.0, look.height * 0.5, 0.0);
            let base = Transform::from_translation(eye).looking_at(target, rot * Vec3::Y);
            base.with_rotation(base.rotation * head)
        }
        Shot::Chase => {
            let centre = Vec3::ZERO;
            let up = view.seat_rot * Vec3::Y;
            let eye = centre + view.ship_rot * orbit;
            Transform::from_translation(eye).looking_at(centre, up)
        }
    };
}

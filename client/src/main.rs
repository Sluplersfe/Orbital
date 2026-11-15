//! Orbital client, phase 2: single-player slice (builder, cockpit, flight report, research).
use bevy::prelude::*;
use orbital_client::parts::{self, CATALOG};
use orbital_client::progress::{Progress, ERA_NAMES};
use orbital_client::report::FlightReport;
use orbital_client::sim::{Flight, Status};
use orbital_client::vessel::{orbit_dv_budget, Vessel};
use orbital_client::{R_EARTH, SPACE_ALT};

enum Mode {
    Builder,
    Research,
    Flying(Box<Flight>),
    Report(Box<FlightReport>, Vec<String>),
}

#[derive(Resource)]
struct Game {
    mode: Mode,
    progress: Progress,
    vessel: Vessel,
    stage: usize,
    pick: usize,
    warp: f64,
    note: String,
}

#[derive(Component)]
struct Hud;

fn main() {
    let mut vessel = Vessel::default();
    vessel.stages = vec![vec!["spark", "tank_m"], vec!["pod1", "chute1"]];
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "Orbital".into(), ..default() }),
            ..default()
        }))
        .insert_resource(Game {
            mode: Mode::Builder, progress: Progress::new(), vessel, stage: 0, pick: 0, warp: 1.0,
            note: "Welcome. Build a rocket, then press L to launch.".into(),
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (input, fly, hud, draw).chain())
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont { font_size: FontSize::Px(16.0), ..default() },
        Node { position_type: PositionType::Absolute, left: Val::Px(12.0), top: Val::Px(8.0), ..default() },
    ));
}

fn unlocked_parts(g: &Game) -> Vec<&'static parts::Part> {
    CATALOG.iter().filter(|p| g.progress.has(p.id)).collect()
}

fn input(keys: Res<ButtonInput<KeyCode>>, mut g: ResMut<Game>) {
    let g = &mut *g;
    let pressed = |k| keys.just_pressed(k);
    match &mut g.mode {
        Mode::Builder => {
            let avail = unlocked_parts(g);
            if pressed(KeyCode::ArrowRight) && !avail.is_empty() { g.pick = (g.pick + 1) % avail.len(); }
            if pressed(KeyCode::ArrowLeft) && !avail.is_empty() { g.pick = (g.pick + avail.len() - 1) % avail.len(); }
            if pressed(KeyCode::ArrowUp) && g.stage + 1 < g.vessel.stages.len() { g.stage += 1; }
            if pressed(KeyCode::ArrowDown) && g.stage > 0 { g.stage -= 1; }
            if pressed(KeyCode::Enter) {
                if let Some(p) = avail.get(g.pick) {
                    if g.vessel.stages.is_empty() { g.vessel.stages.push(vec![]); }
                    let s = g.stage.min(g.vessel.stages.len() - 1);
                    g.vessel.stages[s].push(p.id);
                }
            }
            if pressed(KeyCode::Backspace) {
                let s = g.stage.min(g.vessel.stages.len().saturating_sub(1));
                if let Some(st) = g.vessel.stages.get_mut(s) { st.pop(); }
            }
            if pressed(KeyCode::KeyN) { g.vessel.stages.push(vec![]); g.stage = g.vessel.stages.len() - 1; }
            if pressed(KeyCode::KeyX) {
                if g.vessel.stages.len() > 1 { g.vessel.stages.remove(g.stage); g.stage = g.stage.min(g.vessel.stages.len() - 1); }
            }
            if pressed(KeyCode::KeyR) { g.mode = Mode::Research; g.pick = 0; }
            if pressed(KeyCode::KeyL) {
                g.vessel.stages.retain(|s| !s.is_empty());
                let errs = g.vessel.validate();
                if errs.is_empty() {
                    g.warp = 1.0;
                    g.mode = Mode::Flying(Box::new(Flight::new(g.vessel.clone())));
                } else {
                    g.note = errs.join(" ");
                }
            }
        }
        Mode::Research => {
            let locked: Vec<_> = CATALOG.iter().filter(|p| !g.progress.has(p.id)).collect();
            if pressed(KeyCode::ArrowDown) && !locked.is_empty() { g.pick = (g.pick + 1) % locked.len(); }
            if pressed(KeyCode::ArrowUp) && !locked.is_empty() { g.pick = (g.pick + locked.len() - 1) % locked.len(); }
            if pressed(KeyCode::Enter) {
                if let Some(p) = locked.get(g.pick) {
                    g.note = if g.progress.unlock(p.id) { format!("Unlocked {}.", p.name) }
                        else if p.era > g.progress.era() { format!("{} belongs to era {}: finish the era {} goal first.", p.name, p.era, g.progress.era()) }
                        else { format!("Need {} research points.", p.cost_rp) };
                }
            }
            if pressed(KeyCode::Escape) || pressed(KeyCode::KeyR) { g.mode = Mode::Builder; g.pick = 0; }
        }
        Mode::Flying(f) => {
            let held = |k| keys.pressed(k);
            if held(KeyCode::KeyZ) { f.set_throttle(f.throttle + 0.01); }
            if held(KeyCode::KeyX) { f.set_throttle(f.throttle - 0.01); }
            if pressed(KeyCode::KeyF) { f.set_throttle(1.0); }
            if pressed(KeyCode::KeyC) { f.set_throttle(0.0); }
            if held(KeyCode::KeyD) || held(KeyCode::ArrowRight) { f.set_pitch(f.pitch_cmd + 0.5); }
            if held(KeyCode::KeyA) || held(KeyCode::ArrowLeft) { f.set_pitch(f.pitch_cmd - 0.5); }
            if pressed(KeyCode::Space) { f.stage(); }
            if pressed(KeyCode::KeyP) { f.deploy_chute(); }
            if pressed(KeyCode::KeyT) {
                g.warp = match g.warp as u32 { 1 => 5.0, 5 => 25.0, _ => 1.0 };
            }
            if pressed(KeyCode::Escape) { f.end(); }
        }
        Mode::Report(..) => {
            if pressed(KeyCode::Enter) { g.mode = Mode::Builder; g.stage = 0; g.pick = 0; }
        }
    }
}

fn fly(time: Res<Time>, mut g: ResMut<Game>) {
    let g = &mut *g;
    let Mode::Flying(f) = &mut g.mode else { return };
    // Warp only when coasting in vacuum.
    let warp = if f.alt() > 75_000.0 && f.throttle == 0.0 { g.warp } else { 1.0 };
    let dt = (time.delta_secs_f64() * warp).min(2.0);
    let step = if warp > 1.0 { 0.25 } else { 0.02 };
    f.advance(dt, step);
    if f.status.is_over() {
        let report = FlightReport::from_flight(f);
        let (pts, why) = g.progress.award(&report);
        let mut lines = vec![format!("Research: +{pts} points ({} total)", g.progress.rp)];
        lines.extend(why);
        g.mode = Mode::Report(Box::new(report), lines);
    }
}

fn hud(g: Res<Game>, mut q: Query<&mut Text, With<Hud>>) {
    let Ok(mut text) = q.single_mut() else { return };
    text.0 = match &g.mode {
        Mode::Builder => builder_text(&g),
        Mode::Research => research_text(&g),
        Mode::Flying(f) => cockpit_text(f, g.warp),
        Mode::Report(r, lines) => report_text(r, lines),
    };
}

fn builder_text(g: &Game) -> String {
    let mut s = format!("BUILDER   Era {} ({})   Research points: {}\n\n", g.progress.era(), ERA_NAMES[g.progress.era() as usize - 1], g.progress.rp);
    let avail = unlocked_parts(g);
    s += "Parts (Left/Right to pick, Enter to add to stage):\n";
    for (i, p) in avail.iter().enumerate() {
        let mark = if i == g.pick { ">" } else { " " };
        let detail = match p.kind {
            parts::Kind::Engine { thrust_kn, isp_sl, isp_vac } => format!("{thrust_kn:.0} kN, Isp {isp_sl:.0}/{isp_vac:.0} s"),
            parts::Kind::Tank { prop_kg } => format!("{prop_kg:.0} kg propellant"),
            _ => String::new(),
        };
        s += &format!(" {mark} {:<16} {:>6.0} kg  max-Q {:>3.0} kPa  {}\n", p.name, p.mass, p.strength_pa / 1e3, detail);
    }
    s += "\nStages (Up/Down select, stage 1 fires first, N new stage, X delete stage, Backspace remove part):\n";
    let stats = g.vessel.stats();
    for (i, st) in g.vessel.stages.iter().enumerate() {
        let mark = if i == g.stage { ">" } else { " " };
        let names: Vec<_> = st.iter().filter_map(|id| parts::get(id)).map(|p| p.name).collect();
        let x = stats[i];
        s += &format!(" {mark} Stage {}: {}\n      mass {:.0} kg  dv {:.0} m/s  TWR {:.2}  burn {:.0} s\n", i + 1, if names.is_empty() { "(empty)".into() } else { names.join(", ") }, x.m0, x.dv_vac, x.twr_sl, x.burn_s);
    }
    let dv = g.vessel.total_dv();
    s += &format!("\nTotal delta-v {:.0} m/s   Orbit needs about {:.0} m/s   Wet mass {:.0} kg\n", dv, orbit_dv_budget(), g.vessel.total_mass());
    if let Some((n, q)) = g.vessel.weakest() { s += &format!("Weakest part: {} ({:.0} kPa max-Q)\n", n, q / 1e3); }
    s += &format!("\n{}\n\nL launch   R research", g.note);
    s
}

fn research_text(g: &Game) -> String {
    let mut s = format!("RESEARCH   Points: {}   Era {}\n\nLocked parts (Up/Down, Enter to unlock, Esc back):\n", g.progress.rp, g.progress.era());
    for (i, p) in CATALOG.iter().filter(|p| !g.progress.has(p.id)).enumerate() {
        s += &format!(" {} {:<16} era {}  {:>3} RP\n", if i == g.pick { ">" } else { " " }, p.name, p.era, p.cost_rp);
    }
    s += "\nEarn points: first liftoff, 30 km, space, recovery from space, orbit, and every flight returns data.\n";
    s += &format!("Era 2 (Orbit) opens when you recover a vessel from space.\n\n{}", g.note);
    s
}

fn cockpit_text(f: &Flight, warp: f64) -> String {
    let st = match f.status { Status::OnPad => "ON PAD", _ => "FLYING" };
    let el = f.elements();
    let up = f.pos.normalize();
    let vspeed = f.vel.dot(up);
    let mut s = format!("COCKPIT  {st}   T+{:.0} s   warp x{}\n\n", f.t, if f.alt() > 75_000.0 && f.throttle == 0.0 { warp } else { 1.0 });
    s += &format!("Altitude  {:>9.1} km     Speed {:>7.0} m/s (surface)   Vertical {:>6.0} m/s\n", f.alt() / 1e3, f.rel_vel().length(), vspeed);
    s += &format!("Dynamic pressure {:>5.1} kPa   Mass {:.0} kg\n", f.q() / 1e3, f.mass());
    s += &format!("Throttle {:>3.0}%   Pitch {:>4.0} deg (cmd {:>4.0}; 0 = up, 90 = horizon)\n", f.throttle * 100.0, f.pitch, f.pitch_cmd);
    s += &format!("Stage {} propellant {:>8.0} kg\n", f.stage + 1, f.prop);
    if f.progress_unlocked_computer() {
        s += &format!("Apoapsis {:>8.1} km   Periapsis {:>8.1} km   e {:.3}\n", (el.apoapsis_r - R_EARTH) / 1e3, (el.periapsis_r - R_EARTH) / 1e3, el.e);
    }
    s += &format!("\nZ/X throttle, F full, C cut, A/D pitch, Space stage, P parachute, T warp, Esc end flight\n");
    if let Some(e) = f.events.last() { s += &format!("\nLast: {}", e.msg); }
    s
}

fn report_text(r: &FlightReport, lines: &[String]) -> String {
    let mut s = format!("FLIGHT REPORT\n\n{}\n\n", r.headline);
    s += &format!("Peak {:.1} km   Max speed {:.0} m/s   Max-Q {:.1} kPa   Delta-v used {:.0} of {:.0} m/s   Duration {:.0} s\n\nWhat happened and why:\n", r.max_alt / 1e3, r.max_speed, r.max_q / 1e3, r.dv_used, r.dv_design, r.duration);
    for d in &r.diagnosis { s += &format!(" - {d}\n"); }
    s += "\nTimeline:\n";
    for t in r.timeline.iter().take(16) { s += &format!("  {t}\n"); }
    s += "\n";
    for l in lines { s += &format!("{l}\n"); }
    s += "\nEnter: back to the builder";
    s
}

/// Era 1 cockpit has no predicted orbit; the readout shows once a periapsis is meaningful.
trait Computer { fn progress_unlocked_computer(&self) -> bool; }
impl Computer for Flight {
    fn progress_unlocked_computer(&self) -> bool { self.alt() > 20_000.0 }
}

fn draw(g: Res<Game>, mut gizmos: Gizmos, windows: Query<&Window>) {
    let Mode::Flying(f) = &g.mode else { return };
    let Ok(w) = windows.single() else { return };
    // Map: Earth at lower right, scaled so a 400 km altitude fits.
    let scale = (w.height().min(w.width()) as f64 * 0.12) / R_EARTH;
    let centre = Vec2::new(w.width() * 0.25, -w.height() * 0.1);
    gizmos.circle_2d(Isometry2d::from_translation(centre), (R_EARTH * scale) as f32, Color::srgb(0.2, 0.5, 0.9));
    gizmos.circle_2d(Isometry2d::from_translation(centre), ((R_EARTH + SPACE_ALT) * scale) as f32, Color::srgb(0.2, 0.2, 0.3));
    let p = Vec2::new((f.pos.x * scale) as f32, (f.pos.y * scale) as f32) + centre;
    gizmos.circle_2d(Isometry2d::from_translation(p), 4.0, Color::srgb(1.0, 0.6, 0.1));
}

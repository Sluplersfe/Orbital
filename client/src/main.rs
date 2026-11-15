//! Orbital client, phase 2: single-player slice (builder, cockpit, flight report, research).
use bevy::prelude::*;
use orbital_client::parts;
use orbital_client::autopilot::{Descent, Takeoff};
use orbital_client::program::{Loc, Program};
use orbital_client::space::Place;
use orbital_client::progress::ERA_NAMES;
use orbital_client::report::FlightReport;
use orbital_client::sim::{Flight, Status};
use orbital_client::vessel::{orbit_dv_budget, Vessel};
use orbital_client::net::{self, pb, NetClient, World};
use orbital_client::{R_EARTH, SPACE_ALT};

/// Co-op session, present when ORBITAL_SERVER and ORBITAL_TOKEN are set.
#[derive(Resource)]
struct Online {
    client: NetClient,
    world: World,
    last_send: f64,
    applied_version: u64,
}

fn connect_from_env() -> Option<Online> {
    let server = std::env::var("ORBITAL_SERVER").ok()?;
    let token = std::env::var("ORBITAL_TOKEN").ok()?;
    let host = server.rsplit_once(':').map_or(server.as_str(), |(h, _)| h).to_string();
    let tcp = if server.contains(':') { server.clone() } else { format!("{server}:4004") };
    let udp = std::env::var("ORBITAL_UDP").unwrap_or(format!("{host}:4005"));
    match NetClient::connect(&tcp, &udp, &token) {
        Ok(client) => {
            let world = World::new(&client.ack);
            info!("joined {tcp} as {}", world.me);
            Some(Online { client, world, last_send: 0.0, applied_version: 0 })
        }
        Err(e) => {
            error!("could not join {tcp}: {e}");
            None
        }
    }
}

enum Mode {
    Builder,
    Research,
    Control,
    Flying(Box<Flight>),
    Report(Box<FlightReport>, Vec<String>),
}

#[derive(Resource)]
struct Game {
    mode: Mode,
    program: Program,
    /// Who we are in the program, and when the current flight left the pad (world seconds).
    me: String,
    t0: f64,
    ship_pick: usize,
    dest_pick: usize,
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
    let mut app = App::new();
    let online = connect_from_env();
    let me = online.as_ref().map_or_else(me_name, |o| o.world.me.clone());
    if let Some(online) = online {
        app.insert_resource(online);
    }
    app
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "Orbital".into(), ..default() }),
            ..default()
        }))
        .insert_resource(Game {
            mode: Mode::Builder, program: new_program(&me), me, t0: 0.0, ship_pick: 0, dest_pick: 0, vessel, stage: 0, pick: 0, warp: 1.0,
            note: "Welcome. Build a rocket, then press L to launch.".into(),
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (net_sync, input, fly, hud, draw).chain())
        .run();
}

fn me_name() -> String {
    std::env::var("ORBITAL_NAME").unwrap_or_else(|_| "you".into())
}

fn new_program(me: &str) -> Program {
    let mut p = Program::new();
    let _ = p.add_player(me);
    p
}

/// Who rides a launch: the player, if they are at Earth and the vessel has a seat.
fn launch_crew(g: &Game) -> Vec<String> {
    if g.vessel.crew_capacity() > 0 && g.program.locate(&g.me) == Loc::Earth { vec![g.me.clone()] } else { vec![] }
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
    parts::catalog().iter().filter(|p| g.program.progress.has(p.id) && p.placeable()).collect()
}

fn input(keys: Res<ButtonInput<KeyCode>>, mut g: ResMut<Game>, online: Option<Res<Online>>) {
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
            if pressed(KeyCode::KeyM) { g.mode = Mode::Control; g.note.clear(); }
            if pressed(KeyCode::KeyL) {
                g.vessel.stages.retain(|s| !s.is_empty());
                let errs = g.vessel.validate();
                let crew = launch_crew(g);
                let errs = if errs.is_empty() { g.program.check_launch_crew(&g.vessel, &crew).err().into_iter().collect() } else { errs };
                if errs.is_empty() {
                    g.warp = 1.0;
                    g.t0 = g.program.time;
                    g.mode = Mode::Flying(Box::new(Flight::launch(g.vessel.clone(), g.t0)));
                    if let Some(o) = &online {
                        let _ = o.client.claim(0, pb::CrewRole::Pilot);
                    }
                } else {
                    g.note = errs.join(" ");
                }
            }
        }
        Mode::Research => {
            let locked: Vec<_> = parts::catalog().iter().filter(|p| !g.program.progress.has(p.id)).collect();
            if pressed(KeyCode::ArrowDown) && !locked.is_empty() { g.pick = (g.pick + 1) % locked.len(); }
            if pressed(KeyCode::ArrowUp) && !locked.is_empty() { g.pick = (g.pick + locked.len() - 1) % locked.len(); }
            if pressed(KeyCode::Enter) {
                if let Some(p) = locked.get(g.pick) {
                    g.note = match (g.program.progress.blocker(p.id), &online) {
                        (Some(why), _) => why,
                        // Online the server spends the group's points; our copy updates when it answers.
                        (None, Some(o)) => { let _ = o.client.unlock(p.id, p.cost_rp); format!("Asked the server to unlock {}.", p.name) }
                        (None, None) => { g.program.progress.unlock(p.id); format!("Unlocked {}.", p.name) }
                    };
                }
            }
            if pressed(KeyCode::Escape) || pressed(KeyCode::KeyR) { g.mode = Mode::Builder; g.pick = 0; }
        }
        Mode::Control => control_input(g, &keys),
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

/// Pull server events, and publish our ship's state at 10 Hz while piloting.
fn net_sync(time: Res<Time>, mut g: ResMut<Game>, online: Option<ResMut<Online>>) {
    let Some(mut o) = online else { return };
    let o = &mut *o;
    for ev in o.client.poll() {
        o.world.apply(ev);
    }
    for (key, pts) in std::mem::take(&mut g.program.progress.new_awards) {
        let _ = o.client.award(key, pts);
    }
    if o.world.program.version != o.applied_version {
        o.applied_version = o.world.program.version;
        let p = &o.world.program;
        g.program.progress.apply_shared(p.rp, &p.unlocked, &p.milestones, &p.claims);
        g.program.progress.new_awards.clear();
    }
    if let Some(n) = o.world.notes.pop() {
        g.note = n;
        o.world.notes.clear();
    }
    if !o.world.error.is_empty() {
        g.note = format!("Server: {}", std::mem::take(&mut o.world.error));
    }
    let now = time.elapsed_secs_f64();
    match (&g.mode, o.world.my_ship) {
        (Mode::Flying(f), Some(id)) if now - o.last_send >= 0.1 => {
            o.last_send = now;
            let _ = o.client.send_state(pb::ShipState {
                ship_id: id, pilot: String::new(), world_time: o.world.world_time(),
                position: Some(net::v3(f.pos)), velocity: Some(net::v3(f.vel)),
            });
        }
        (Mode::Flying(_), _) => {}
        (_, Some(id)) => {
            // Flight over: leave the pilot seat (an orbiting ship stays on rails).
            let _ = o.client.send(pb::envelope::Msg::SeatRelease(pb::SeatRelease { ship_id: id }));
            o.world.my_ship = None;
        }
        _ => {}
    }
}

fn online_text(o: &Online) -> String {
    let w = &o.world;
    if w.closed {
        return "\n\nOFFLINE: lost connection to the server".into();
    }
    let role = if w.is_host() { " (you are host)" } else { "" };
    format!("\n\nCO-OP as {}{}   host {}   players {}   other ships {}   world T {:.0} s",
        w.me, role, w.host, w.roster.players.len(), w.others().len(), w.world_time())
}

fn fly(time: Res<Time>, mut g: ResMut<Game>, online: Option<Res<Online>>) {
    let g = &mut *g;
    let Mode::Flying(f) = &mut g.mode else { return };
    // Warp only when coasting in vacuum, and never in co-op: the world clock is shared.
    let warp = if online.is_none() && f.alt() > 75_000.0 && f.throttle == 0.0 { g.warp } else { 1.0 };
    let dt = (time.delta_secs_f64() * warp).min(2.0);
    let step = if warp > 1.0 { 0.25 } else { 0.02 };
    f.advance(dt, step);
    if f.status.is_over() {
        let name = format!("Ship {}", g.program.next_id);
        let crew = if f.vessel.crew_capacity() > 0 && g.program.locate(&g.me) == Loc::Earth { vec![g.me.clone()] } else { vec![] };
        let res = g.program.finish_flight(&name, f, g.t0, &crew);
        let mut lines = vec![format!("Research: {} points in total", g.program.progress.rp)];
        lines.extend(res.awards);
        if res.ship.is_some() {
            lines.push("The ship is in orbit: press M for mission control.".into());
        }
        g.mode = Mode::Report(Box::new(res.report), lines);
    }
}

fn hud(g: Res<Game>, online: Option<Res<Online>>, mut q: Query<&mut Text, With<Hud>>) {
    let Ok(mut text) = q.single_mut() else { return };
    let net = online.map_or(String::new(), |o| online_text(&o));
    text.0 = match &g.mode {
        Mode::Builder => builder_text(&g),
        Mode::Research => research_text(&g),
        Mode::Control => control_text(&g),
        Mode::Flying(f) => cockpit_text(f, g.warp),
        Mode::Report(r, lines) => report_text(r, lines),
    } + &net;
}

fn builder_text(g: &Game) -> String {
    let mut s = format!("BUILDER   Era {} ({})   Research points: {}\n\n", g.program.progress.era(), ERA_NAMES[g.program.progress.era() as usize - 1], g.program.progress.rp);
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
    s += &format!("\n{}\n\nL launch   R research   M mission control", g.note);
    s
}

fn research_text(g: &Game) -> String {
    let mut s = format!("RESEARCH   Points: {}   Era {}\n\nLocked parts (Up/Down, Enter to unlock, Esc back):\n", g.program.progress.rp, g.program.progress.era());
    for (i, p) in parts::catalog().iter().filter(|p| !g.program.progress.has(p.id)).enumerate() {
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

fn draw(g: Res<Game>, online: Option<Res<Online>>, mut gizmos: Gizmos, windows: Query<&Window>) {
    let Mode::Flying(f) = &g.mode else { return };
    let Ok(w) = windows.single() else { return };
    // Map: Earth at lower right, scaled so a 400 km altitude fits.
    let scale = (w.height().min(w.width()) as f64 * 0.12) / R_EARTH;
    let centre = Vec2::new(w.width() * 0.25, -w.height() * 0.1);
    gizmos.circle_2d(Isometry2d::from_translation(centre), (R_EARTH * scale) as f32, Color::srgb(0.2, 0.5, 0.9));
    gizmos.circle_2d(Isometry2d::from_translation(centre), ((R_EARTH + SPACE_ALT) * scale) as f32, Color::srgb(0.2, 0.2, 0.3));
    let p = Vec2::new((f.pos.x * scale) as f32, (f.pos.y * scale) as f32) + centre;
    gizmos.circle_2d(Isometry2d::from_translation(p), 4.0, Color::srgb(1.0, 0.6, 0.1));
    if let Some(o) = online {
        for (_, pos) in o.world.others() {
            let q = Vec2::new((pos.x * scale) as f32, (pos.y * scale) as f32) + centre;
            gizmos.circle_2d(Isometry2d::from_translation(q), 3.0, Color::srgb(0.3, 1.0, 0.4));
        }
    }
}

fn place_text(p: &Place) -> String {
    match p {
        Place::Orbit(o) => format!("orbit of {} at {:.0} km", o.body, (o.r - o.body().radius) / 1e3),
        Place::Landed { body, .. } => format!("landed on {body}"),
        Place::Docked { station } => format!("docked to station {station}"),
        Place::Transit { to, .. } => format!("on the way to {to}"),
        Place::Destroyed => "destroyed".into(),
    }
}

fn control_text(g: &Game) -> String {
    let p = &g.program;
    let era = p.progress.era();
    let mut s = format!("MISSION CONTROL   Era {} ({})   Research {}   T+{:.1} h   You: {} at {:?}\n", era, ERA_NAMES[era as usize - 1], p.progress.rp, p.time / 3600.0, g.me, p.locate(&g.me));
    s += &format!("Claims: {}\n\nStations:\n", if p.progress.claims.is_empty() { "none".into() } else { p.progress.claims.iter().cloned().collect::<Vec<_>>().join(", ") });
    for st in &p.stations {
        s += &format!("  #{} {}  crew {:?}  fuel {:.0} kg  supplies {:.0} h  power {}\n", st.id, st.name, st.crew, st.prop_kg, st.supply_hours().min(9999.0), if st.powered() { "ok" } else { "OUT" });
    }
    s += "\nShips (Up/Down):\n";
    for (i, sh) in p.ships.iter().enumerate() {
        let mark = if i == g.ship_pick % p.ships.len().max(1) { ">" } else { " " };
        s += &format!(" {mark} #{} {}  {}  crew {:?}  dv {:.0} m/s\n", sh.id, sh.name, place_text(&sh.place), sh.crew, sh.dv_remaining());
    }
    let dests = destinations(g);
    if let Some(d) = dests.get(g.dest_pick % dests.len().max(1)) {
        s += &format!("\nDestination (Left/Right): {}\n", d);
    }
    s += "\nB found station   D dock   A attach   U undock   F refuel   G board/leave ship   T transfer   W wait 1 h\n";
    s += "L land   K take off   V survey   Z teleport home   Esc back\n\nLog:\n";
    for l in p.log.iter().rev().take(8).rev() { s += &format!("  {l}\n"); }
    s += &format!("\n{}", g.note);
    s
}

fn destinations(g: &Game) -> Vec<&'static str> {
    let era = g.program.progress.era();
    orbital_client::bodies::bodies().iter().filter(|b| b.id != "sun" && b.min_era <= era).map(|b| b.id).collect()
}

fn control_input(g: &mut Game, keys: &ButtonInput<KeyCode>) {
    let pressed = |k| keys.just_pressed(k);
    let nships = g.program.ships.len();
    if pressed(KeyCode::ArrowDown) && nships > 0 { g.ship_pick = (g.ship_pick + 1) % nships; }
    if pressed(KeyCode::ArrowUp) && nships > 0 { g.ship_pick = (g.ship_pick + nships - 1) % nships; }
    let nd = destinations(g).len().max(1);
    if pressed(KeyCode::ArrowRight) { g.dest_pick = (g.dest_pick + 1) % nd; }
    if pressed(KeyCode::ArrowLeft) { g.dest_pick = (g.dest_pick + nd - 1) % nd; }
    if pressed(KeyCode::Escape) { g.mode = Mode::Builder; return; }
    let ship = g.program.ships.get(g.ship_pick % nships.max(1)).map(|s| s.id);
    let station = g.program.stations.first().map(|s| s.id);
    let me = g.me.clone();
    let say = |g: &mut Game, r: Result<String, String>| g.note = r.unwrap_or_else(|e| e);
    if pressed(KeyCode::KeyW) { g.program.tick(3600.0); g.note = "An hour passes.".into(); }
    if pressed(KeyCode::KeyZ) { let r = g.program.teleport_to_earth(&me).map(|_| "Back at Earth.".into()); say(g, r); }
    let Some(id) = ship else { return };
    if pressed(KeyCode::KeyB) { let r = g.program.found_station(id, &format!("Station {id}")).map(|s| format!("Founded station {s}.")); say(g, r); }
    if pressed(KeyCode::KeyD) {
        let r = match station { Some(st) => g.program.rendezvous(id, st).map(|dv| format!("Docked, {dv:.0} m/s spent.")), None => Err("There is no station.".into()) };
        say(g, r);
    }
    if pressed(KeyCode::KeyA) { let r = g.program.attach(id).map(|_| "Folded into the station.".into()); say(g, r); }
    if pressed(KeyCode::KeyU) { let r = g.program.undock(id).map(|_| "Undocked.".into()); say(g, r); }
    if pressed(KeyCode::KeyF) { let r = g.program.refuel(id).map(|k| format!("Took {k:.0} kg of propellant.")); say(g, r); }
    if pressed(KeyCode::KeyG) {
        let to = match g.program.locate(&me) { Loc::Station(_) => Some(Loc::Ship(id)), Loc::Ship(_) => station.map(Loc::Station), Loc::Earth => None };
        let r = match to { Some(t) => g.program.move_crew(&me, t).map(|_| "Moved.".into()), None => Err("You are at Earth: launch, or teleport to a friend.".into()) };
        say(g, r);
    }
    if pressed(KeyCode::KeyT) {
        let dests = destinations(g);
        let r = match dests.get(g.dest_pick % dests.len().max(1)) {
            Some(d) => g.program.transfer(id, d).map(|eta| { let t = eta + 60.0; g.program.tick(t); format!("Arrived after {:.1} h.", t / 3600.0) }),
            None => Err("Nowhere to go.".into()),
        };
        say(g, r);
    }
    if pressed(KeyCode::KeyL) {
        let r = g.program.begin_descent(id).and_then(|(mut f, t0)| {
            Descent::new().run(&mut f, 6_000.0);
            g.program.finish_descent(id, &f, t0).map(|r| r.report.headline)
        });
        say(g, r);
    }
    if pressed(KeyCode::KeyK) {
        let r = g.program.begin_takeoff(id).and_then(|(mut f, t0)| {
            Takeoff::new(60_000.0).run(&mut f, 3_000.0);
            g.program.finish_ascent(id, &f, t0).map(|r| r.report.headline)
        });
        say(g, r);
    }
    if pressed(KeyCode::KeyV) { let r = g.program.survey(id).map(|l| l.join(" ")); say(g, r); }
}

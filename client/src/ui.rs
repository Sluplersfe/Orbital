//! Screen panels over the 3D view: the top bar, the builder's parts list, delta-v readout and
//! staging, the research tree, mission control, the flight instruments and the flight report.
//!
//! Panels are rebuilt when the state they show changes (see `signature`); in flight the layout is
//! fixed and `live` rewrites the readouts every frame. Every button maps to the same key the
//! keyboard uses, so clicking and typing go through one input path (`VirtualKeys`).
use crate::{destinations, launch_crew, place_text, unlocked_parts, Game, Mode, Online};
use bevy::picking::events::{PointerClick, PointerOut, PointerOver};
use bevy::prelude::*;
use orbital_client::parts::{self, Kind};
use orbital_client::progress::ERA_NAMES;
use orbital_client::report::FlightReport;
use orbital_client::sim::{Flight, Status};
use orbital_client::vessel::orbit_dv_budget;
use orbital_client::{R_EARTH, SPACE_ALT};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Keys pressed by clicking a button this frame; `input` treats them as real key presses.
#[derive(Resource, Default)]
pub struct VirtualKeys(pub Vec<KeyCode>);

#[derive(Resource, Default)]
pub struct UiState {
    sig: u64,
    /// Builder part category.
    pub cat: usize,
}

#[derive(Component)]
struct UiRoot;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Builder,
    Research,
    Control,
}

/// What a button does when clicked.
#[derive(Component, Clone, Copy)]
pub enum Act {
    Key(KeyCode),
    /// Pick a part (index into the unlocked list) and add it to the selected stage.
    Part(usize),
    Stage(usize),
    /// Pick a locked part (index into the locked list) and research it.
    Research(usize),
    Ship(usize),
    Tab(Tab),
    Cat(usize),
}

#[derive(Component)]
struct Tint {
    base: Color,
    hover: Color,
}

/// A readout rewritten every frame in flight.
#[derive(Component, Clone, Copy)]
enum Live {
    Time,
    Alt,
    Speed,
    VSpeed,
    Apo,
    Peri,
    Twr,
    Dv,
    Fuel,
    FuelBar,
    Mass,
    Q,
    QBar,
    Throttle,
    ThrottleBar,
    Event,
    Check(usize),
    Profile(usize),
    Warp,
}

const BG: Color = Color::srgba(0.06, 0.09, 0.14, 0.86);
const LINE: Color = Color::srgba(0.35, 0.45, 0.6, 0.45);
const BTN: Color = Color::srgba(0.13, 0.18, 0.26, 0.95);
const BTN_HOVER: Color = Color::srgba(0.2, 0.3, 0.45, 1.0);
const ACCENT: Color = Color::srgb(0.25, 0.55, 0.95);
const GREEN: Color = Color::srgb(0.3, 0.85, 0.4);
const AMBER: Color = Color::srgb(0.95, 0.7, 0.2);
const RED: Color = Color::srgb(0.95, 0.3, 0.25);
const DIM: Color = Color::srgb(0.6, 0.65, 0.72);
const STAGE_COLORS: [Color; 6] = [
    Color::srgb(0.3, 0.85, 0.4),
    Color::srgb(0.95, 0.6, 0.15),
    Color::srgb(0.25, 0.7, 0.95),
    Color::srgb(0.8, 0.4, 0.9),
    Color::srgb(0.95, 0.85, 0.3),
    Color::srgb(0.9, 0.35, 0.35),
];

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VirtualKeys>()
            .init_resource::<UiState>()
            .add_observer(click)
            .add_observer(hover)
            .add_observer(unhover)
            .add_systems(Update, (rebuild, live).chain().after(crate::GameSet));
    }
}

fn click(ev: On<PointerClick>, acts: Query<&Act>, mut g: ResMut<Game>, mut vk: ResMut<VirtualKeys>, mut ui: ResMut<UiState>) {
    let Ok(act) = acts.get(ev.entity) else { return };
    let g = &mut *g;
    match *act {
        Act::Key(k) => vk.0.push(k),
        Act::Part(i) => {
            g.pick = i;
            vk.0.push(KeyCode::Enter);
        }
        Act::Stage(i) => g.stage = i,
        Act::Research(i) => {
            g.pick = i;
            vk.0.push(KeyCode::Enter);
        }
        Act::Ship(i) => g.ship_pick = i,
        Act::Cat(c) => ui.cat = c,
        Act::Tab(t) => {
            if matches!(g.mode, Mode::Builder | Mode::Research | Mode::Control) {
                g.pick = 0;
                g.mode = match t {
                    Tab::Builder => Mode::Builder,
                    Tab::Research => Mode::Research,
                    Tab::Control => Mode::Control,
                };
            }
        }
    }
}

fn hover(ev: On<PointerOver>, mut q: Query<(&Tint, &mut BackgroundColor)>) {
    if let Ok((t, mut bg)) = q.get_mut(ev.entity) {
        bg.0 = t.hover;
    }
}

fn unhover(ev: On<PointerOut>, mut q: Query<(&Tint, &mut BackgroundColor)>) {
    if let Ok((t, mut bg)) = q.get_mut(ev.entity) {
        bg.0 = t.base;
    }
}

fn signature(g: &Game, ui: &UiState, online: Option<&Online>) -> u64 {
    let mut h = DefaultHasher::new();
    let p = &g.program;
    match &g.mode {
        Mode::Builder => ("builder", g.pick, g.stage, format!("{:?}", g.vessel.stages), ui.cat).hash(&mut h),
        Mode::Research => ("research", g.pick).hash(&mut h),
        Mode::Control => ("control", g.ship_pick, g.dest_pick, p.log.len(), p.time as u64, format!("{:?}", p.locate(&g.me))).hash(&mut h),
        Mode::Flying(_) => "flying".hash(&mut h),
        Mode::Report(r, _) => ("report", &r.headline).hash(&mut h),
    }
    (&g.note, p.progress.rp, p.progress.era(), p.progress.claims.len(), p.ships.len(), p.stations.len()).hash(&mut h);
    (0..parts::catalog().len()).filter(|&i| p.progress.has(parts::catalog()[i].id)).count().hash(&mut h);
    if let Some(o) = online {
        (o.world.roster.players.len(), &o.world.host, o.world.closed).hash(&mut h);
    }
    h.finish()
}

fn rebuild(mut commands: Commands, g: Res<Game>, online: Option<Res<Online>>, mut ui: ResMut<UiState>, roots: Query<Entity, With<UiRoot>>) {
    let sig = signature(&g, &ui, online.as_deref());
    if sig == ui.sig && !roots.is_empty() {
        return;
    }
    ui.sig = sig;
    for e in &roots {
        commands.entity(e).despawn();
    }
    let root = commands
        .spawn((UiRoot, Node { width: percent(100), height: percent(100), position_type: PositionType::Absolute, ..default() }, Pickable::IGNORE))
        .id();
    commands.entity(root).with_children(|p| {
        top_bar(p, &g, online.as_deref());
        match &g.mode {
            Mode::Builder => builder(p, &g, &ui),
            Mode::Research => research(p, &g),
            Mode::Control => control(p, &g),
            Mode::Flying(f) => flight(p, f),
            Mode::Report(r, lines) => report(p, r, lines),
        }
    });
}

// ---- building blocks ----

fn text(p: &mut ChildSpawnerCommands, s: impl Into<String>, size: f32, color: Color) {
    p.spawn((Text::new(s), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(color), Pickable::IGNORE));
}

fn live_text(p: &mut ChildSpawnerCommands, l: Live, size: f32, color: Color) {
    p.spawn((Text::new(""), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(color), l, Pickable::IGNORE));
}

/// An absolutely placed panel with a title.
fn panel(p: &mut ChildSpawnerCommands, pos: Node, title: &str, body: impl FnOnce(&mut ChildSpawnerCommands)) {
    p.spawn((
        Node {
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(12)),
            row_gap: px(6),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(8)),
            ..pos
        },
        BackgroundColor(BG),
        BorderColor::all(LINE),
    ))
    .with_children(|p| {
        if !title.is_empty() {
            text(p, title, 19.0, Color::WHITE);
        }
        body(p);
    });
}

fn button(p: &mut ChildSpawnerCommands, label: &str, act: Act, base: Color, size: f32) {
    p.spawn((
        Node { padding: UiRect::axes(px(12), px(7)), border_radius: BorderRadius::all(px(6)), justify_content: JustifyContent::Center, ..default() },
        BackgroundColor(base),
        Tint { base, hover: BTN_HOVER },
        act,
    ))
    .with_children(|p| text(p, label, size, Color::WHITE));
}

fn row(p: &mut ChildSpawnerCommands, body: impl FnOnce(&mut ChildSpawnerCommands)) {
    p.spawn((Node { flex_direction: FlexDirection::Row, column_gap: px(10), align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, ..default() }, Pickable::IGNORE))
        .with_children(body);
}

fn kv(p: &mut ChildSpawnerCommands, k: &str, v: String) {
    row(p, |p| {
        text(p, k, 15.0, DIM);
        text(p, v, 15.0, Color::WHITE);
    });
}

fn bar(p: &mut ChildSpawnerCommands, l: Live, color: Color) {
    p.spawn((Node { width: px(150), height: px(8), border_radius: BorderRadius::all(px(4)), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.12)), Pickable::IGNORE))
        .with_children(|p| {
            p.spawn((Node { width: percent(0), height: percent(100), border_radius: BorderRadius::all(px(4)), ..default() }, BackgroundColor(color), l, Pickable::IGNORE));
        });
}

fn at(left: Option<f32>, right: Option<f32>, top: Option<f32>, bottom: Option<f32>, width: Option<f32>) -> Node {
    let v = |x: Option<f32>| x.map_or(Val::Auto, px);
    Node { left: v(left), right: v(right), top: v(top), bottom: v(bottom), width: v(width), ..default() }
}

// ---- top bar ----

fn top_bar(p: &mut ChildSpawnerCommands, g: &Game, online: Option<&Online>) {
    let active = match g.mode {
        Mode::Builder => 0,
        Mode::Flying(_) | Mode::Report(..) => 1,
        Mode::Research => 2,
        Mode::Control => 3,
    };
    p.spawn((
        Node { position_type: PositionType::Absolute, left: px(0), right: px(0), top: px(0), height: px(52), align_items: AlignItems::Center, padding: UiRect::horizontal(px(16)), column_gap: px(8), ..default() },
        BackgroundColor(Color::srgba(0.04, 0.06, 0.1, 0.9)),
    ))
    .with_children(|p| {
        text(p, "ORBITAL", 24.0, Color::WHITE);
        p.spawn(Node { width: px(24), ..default() });
        let tabs: [(&str, Option<Act>); 4] = [
            ("Builder", Some(Act::Tab(Tab::Builder))),
            ("Flight", None),
            ("Research", Some(Act::Tab(Tab::Research))),
            ("Mission Control", Some(Act::Tab(Tab::Control))),
        ];
        for (i, (label, act)) in tabs.into_iter().enumerate() {
            let base = if i == active { ACCENT } else { Color::NONE };
            match act {
                Some(a) if !matches!(g.mode, Mode::Flying(_) | Mode::Report(..)) => button(p, label, a, base, 17.0),
                _ => {
                    p.spawn((Node { padding: UiRect::axes(px(12), px(7)), border_radius: BorderRadius::all(px(6)), ..default() }, BackgroundColor(base)))
                        .with_children(|p| text(p, label, 17.0, if i == active { Color::WHITE } else { DIM }));
                }
            }
        }
        p.spawn(Node { flex_grow: 1.0, ..default() });
        let era = g.program.progress.era();
        text(p, format!("Era {} {}   {} RP", era, ERA_NAMES[era as usize - 1], g.program.progress.rp), 16.0, DIM);
        p.spawn(Node { width: px(16), ..default() });
        match online {
            Some(o) if o.world.closed => text(p, "offline: connection lost", 16.0, RED),
            Some(o) => {
                let host = if o.world.is_host() { ", you host" } else { "" };
                text(p, format!("{} online", o.world.roster.players.len()), 16.0, GREEN);
                text(p, format!("  {}{}", o.server, host), 16.0, Color::WHITE);
            }
            None => text(p, format!("single player: {}", g.me), 16.0, DIM),
        }
    });
}

// ---- builder ----

const CATS: [&str; 5] = ["Engines", "Tanks", "Structure", "Crew", "Station"];

fn category(k: &Kind) -> usize {
    match k {
        Kind::Engine { .. } => 0,
        Kind::Tank { .. } => 1,
        Kind::Structure | Kind::NoseCone | Kind::Parachute | Kind::Docking => 2,
        Kind::Pod { .. } | Kind::Payload | Kind::Survey | Kind::Supply { .. } => 3,
        _ => 4,
    }
}

fn part_detail(p: &parts::Part) -> Vec<(String, String)> {
    let mut v = vec![("Mass".into(), format!("{:.0} kg", p.mass)), ("Max-Q".into(), format!("{:.0} kPa", p.strength_pa / 1e3))];
    match p.kind {
        Kind::Engine { thrust_kn, isp_sl, isp_vac } => {
            v.push(("Thrust".into(), format!("{thrust_kn:.0} kN")));
            v.push(("Isp (sea level)".into(), format!("{isp_sl:.0} s")));
            v.push(("Isp (vacuum)".into(), format!("{isp_vac:.0} s")));
        }
        Kind::Tank { prop_kg } => v.push(("Propellant".into(), format!("{prop_kg:.0} kg"))),
        Kind::Pod { crew } => v.push(("Seats".into(), format!("{crew}"))),
        Kind::Habitat { crew, recycle } => v.push(("Crew / recycling".into(), format!("{crew} / {:.0}%", recycle * 100.0))),
        Kind::Solar { kw } => v.push(("Power".into(), format!("{kw:.0} kW"))),
        Kind::Battery { kwh } => v.push(("Storage".into(), format!("{kwh:.0} kWh"))),
        Kind::Depot { prop_kg } => v.push(("Depot".into(), format!("{prop_kg:.0} kg"))),
        _ => {}
    }
    v
}

fn short_stat(p: &parts::Part) -> String {
    match p.kind {
        Kind::Engine { thrust_kn, .. } => format!("{thrust_kn:.0} kN"),
        Kind::Tank { prop_kg } => format!("{:.1} t", prop_kg / 1e3),
        Kind::Pod { crew } => format!("{crew} seat"),
        _ => format!("{:.0} kg", p.mass),
    }
}

fn builder(p: &mut ChildSpawnerCommands, g: &Game, ui: &UiState) {
    let avail = unlocked_parts(g);
    panel(p, at(Some(12.0), None, Some(64.0), Some(12.0), Some(340.0)), "Parts", |p| {
        p.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(4), row_gap: px(4), ..default() }).with_children(|p| {
            for (i, c) in CATS.iter().enumerate() {
                button(p, c, Act::Cat(i), if i == ui.cat { ACCENT } else { BTN }, 14.0);
            }
        });
        p.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(6), row_gap: px(6), ..default() }).with_children(|p| {
            for (i, part) in avail.iter().enumerate().filter(|(_, part)| category(&part.kind) == ui.cat) {
                let base = if i == g.pick { Color::srgba(0.18, 0.35, 0.6, 1.0) } else { BTN };
                p.spawn((
                    Node { width: px(150), flex_direction: FlexDirection::Column, padding: UiRect::all(px(8)), border_radius: BorderRadius::all(px(6)), border: UiRect::all(px(if i == g.pick { 2 } else { 0 })), ..default() },
                    BackgroundColor(base),
                    BorderColor::all(ACCENT),
                    Tint { base, hover: BTN_HOVER },
                    Act::Part(i),
                ))
                .with_children(|p| {
                    text(p, part.name, 15.0, Color::WHITE);
                    text(p, short_stat(part), 13.0, DIM);
                });
            }
        });
        if let Some(part) = avail.get(g.pick) {
            p.spawn(Node { height: px(8), ..default() });
            text(p, part.name, 18.0, Color::WHITE);
            for (k, v) in part_detail(part) {
                kv(p, &k, v);
            }
            text(p, "Click a part to add it to the selected stage (Enter adds the highlighted one).", 13.0, DIM);
        }
    });

    let stats = g.vessel.stats();
    panel(p, at(None, Some(12.0), Some(64.0), None, Some(300.0)), "Delta-v Readout", |p| {
        for (i, s) in stats.iter().enumerate().rev() {
            row(p, |p| {
                text(p, format!("Stage {}", i + 1), 15.0, STAGE_COLORS[i % STAGE_COLORS.len()]);
                text(p, format!("{:.0} m/s", s.dv_vac), 15.0, Color::WHITE);
            });
        }
        let dv = g.vessel.total_dv();
        let need = orbit_dv_budget();
        row(p, |p| {
            text(p, "Total", 18.0, Color::WHITE);
            text(p, format!("{dv:.0} m/s"), 18.0, if dv >= need { GREEN } else { AMBER });
        });
        text(p, format!("Orbit needs about {need:.0} m/s"), 13.0, DIM);
    });

    panel(p, at(None, Some(12.0), Some(300.0), None, Some(300.0)), "Staging", |p| {
        for (i, st) in g.vessel.stages.iter().enumerate().rev() {
            let base = if i == g.stage { Color::srgba(0.18, 0.35, 0.6, 1.0) } else { BTN };
            let names: Vec<_> = st.iter().filter_map(|id| parts::get(id)).map(|p| p.name).collect();
            p.spawn((
                Node { flex_direction: FlexDirection::Column, padding: UiRect::all(px(8)), border_radius: BorderRadius::all(px(6)), ..default() },
                BackgroundColor(base),
                Tint { base, hover: BTN_HOVER },
                Act::Stage(i),
            ))
            .with_children(|p| {
                let s = stats.get(i).copied().unwrap_or_default();
                text(p, format!("Stage {}   TWR {:.2}   burn {:.0} s", i + 1, s.twr_sl, s.burn_s), 15.0, STAGE_COLORS[i % STAGE_COLORS.len()]);
                text(p, if names.is_empty() { "(empty)".to_string() } else { names.join(", ") }, 13.0, Color::WHITE);
            });
        }
        p.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(6), row_gap: px(6), ..default() }).with_children(|p| {
            button(p, "+ New stage (N)", Act::Key(KeyCode::KeyN), BTN, 14.0);
            button(p, "Delete stage (X)", Act::Key(KeyCode::KeyX), BTN, 14.0);
            button(p, "Remove part (Bksp)", Act::Key(KeyCode::Backspace), BTN, 14.0);
        });
        text(p, "Stage 1 fires first and sits at the bottom.", 13.0, DIM);
    });

    let s0 = stats.first().copied().unwrap_or_default();
    let errs = g.vessel.validate();
    let crew_err = g.program.check_launch_crew(&g.vessel, &launch_crew(g)).err();
    panel(p, at(Some(364.0), None, None, Some(76.0), Some(330.0)), "Rocket Summary", |p| {
        kv(p, "Total mass", format!("{:.1} t", g.vessel.total_mass() / 1e3));
        kv(p, "Thrust (stage 1)", format!("{:.0} kN", s0.thrust_kn));
        kv(p, "TWR (sea level)", format!("{:.2}", s0.twr_sl));
        kv(p, "Delta-v (vacuum)", format!("{:.0} m/s", g.vessel.total_dv()));
        if let Some((n, q)) = g.vessel.weakest() {
            kv(p, "Weakest part", format!("{n}, {:.0} kPa", q / 1e3));
        }
        match errs.first().cloned().or(crew_err) {
            None => text(p, "Ready to launch", 16.0, GREEN),
            Some(e) => text(p, e, 14.0, AMBER),
        }
    });

    p.spawn(Node { position_type: PositionType::Absolute, left: px(364), bottom: px(16), column_gap: px(10), ..default() }).with_children(|p| {
        button(p, "Launch (L)", Act::Key(KeyCode::KeyL), Color::srgb(0.15, 0.55, 0.25), 18.0);
    });
    note(p, g, "Right-drag to orbit the rocket, wheel to zoom.");
}

fn note(p: &mut ChildSpawnerCommands, g: &Game, hint: &str) {
    panel(p, at(None, Some(12.0), None, Some(16.0), Some(420.0)), "", |p| {
        if !g.note.is_empty() {
            text(p, &g.note, 15.0, Color::WHITE);
        }
        text(p, hint, 13.0, DIM);
    });
}

// ---- research ----

fn research(p: &mut ChildSpawnerCommands, g: &Game) {
    let prog = &g.program.progress;
    let locked: Vec<_> = parts::catalog().iter().filter(|p| !prog.has(p.id)).map(|p| p.id).collect();
    p.spawn((Node { position_type: PositionType::Absolute, left: px(12), right: px(12), top: px(64), bottom: px(110), column_gap: px(10), ..default() }, Pickable::IGNORE))
        .with_children(|p| {
            for branch in parts::BRANCHES {
                p.spawn((
                    Node { flex_grow: 1.0, flex_basis: px(0), flex_direction: FlexDirection::Column, row_gap: px(5), padding: UiRect::all(px(10)), border_radius: BorderRadius::all(px(8)), overflow: Overflow::clip_y(), ..default() },
                    BackgroundColor(BG),
                ))
                .with_children(|p| {
                    text(p, branch.replace('_', " ").to_uppercase(), 17.0, Color::WHITE);
                    for part in parts::catalog().iter().filter(|x| x.branch == branch) {
                        let (base, state) = if prog.has(part.id) {
                            (Color::srgba(0.1, 0.3, 0.15, 0.95), "researched".to_string())
                        } else if let Some(why) = prog.blocker(part.id) {
                            (Color::srgba(0.12, 0.13, 0.16, 0.95), why)
                        } else {
                            (Color::srgba(0.12, 0.25, 0.45, 0.95), format!("{} RP: click to research", part.cost_rp))
                        };
                        let mut e = p.spawn((
                            Node { flex_direction: FlexDirection::Column, padding: UiRect::all(px(6)), border_radius: BorderRadius::all(px(5)), ..default() },
                            BackgroundColor(base),
                        ));
                        if let Some(i) = locked.iter().position(|id| *id == part.id) {
                            e.insert((Act::Research(i), Tint { base, hover: BTN_HOVER }));
                        }
                        e.with_children(|p| {
                            text(p, format!("{}  (era {}, tier {})", part.name, part.era, part.tier), 14.0, Color::WHITE);
                            text(p, state, 12.0, DIM);
                        });
                    }
                });
            }
        });
    note(p, g, "Earn points: first liftoff, 30 km, space, recovery, orbit, surveys. Every flight returns data.");
}

// ---- mission control ----

fn control(p: &mut ChildSpawnerCommands, g: &Game) {
    let prog = &g.program;
    panel(p, at(Some(12.0), None, Some(64.0), Some(12.0), Some(460.0)), "Mission Control", |p| {
        kv(p, "Clock", format!("T+{:.1} h", prog.time / 3600.0));
        kv(p, "You", format!("{} at {:?}", g.me, prog.locate(&g.me)));
        let claims = if prog.progress.claims.is_empty() { "none".into() } else { prog.progress.claims.iter().cloned().collect::<Vec<_>>().join(", ") };
        kv(p, "Claims", claims);
        text(p, "Stations", 17.0, Color::WHITE);
        if prog.stations.is_empty() {
            text(p, "none yet: launch a station core to orbit, then found one (B)", 13.0, DIM);
        }
        for st in &prog.stations {
            text(p, format!("#{} {}  crew {}  fuel {:.0} kg  supplies {:.0} h  power {}", st.id, st.name, st.crew.len(), st.prop_kg, st.supply_hours().min(9999.0), if st.powered() { "ok" } else { "OUT" }), 14.0, if st.powered() { Color::WHITE } else { RED });
        }
        text(p, "Ships", 17.0, Color::WHITE);
        if prog.ships.is_empty() {
            text(p, "none in space yet", 13.0, DIM);
        }
        for (i, sh) in prog.ships.iter().enumerate() {
            let sel = i == g.ship_pick % prog.ships.len().max(1);
            let base = if sel { Color::srgba(0.18, 0.35, 0.6, 1.0) } else { BTN };
            p.spawn((Node { flex_direction: FlexDirection::Column, padding: UiRect::all(px(6)), border_radius: BorderRadius::all(px(5)), ..default() }, BackgroundColor(base), Tint { base, hover: BTN_HOVER }, Act::Ship(i)))
                .with_children(|p| {
                    text(p, format!("#{} {}   dv {:.0} m/s   crew {}", sh.id, sh.name, sh.dv_remaining(), sh.crew.len()), 14.0, Color::WHITE);
                    text(p, place_text(&sh.place), 12.0, DIM);
                });
        }
        let dests = destinations(g);
        if let Some(d) = dests.get(g.dest_pick % dests.len().max(1)) {
            row(p, |p| {
                button(p, "<", Act::Key(KeyCode::ArrowLeft), BTN, 14.0);
                text(p, format!("Destination: {d}"), 15.0, Color::WHITE);
                button(p, ">", Act::Key(KeyCode::ArrowRight), BTN, 14.0);
            });
        }
        p.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(5), row_gap: px(5), ..default() }).with_children(|p| {
            let acts = [
                ("Found station (B)", KeyCode::KeyB),
                ("Dock (D)", KeyCode::KeyD),
                ("Attach (A)", KeyCode::KeyA),
                ("Undock (U)", KeyCode::KeyU),
                ("Refuel (F)", KeyCode::KeyF),
                ("Board / leave (G)", KeyCode::KeyG),
                ("Transfer (T)", KeyCode::KeyT),
                ("Land (L)", KeyCode::KeyL),
                ("Take off (K)", KeyCode::KeyK),
                ("Survey (V)", KeyCode::KeyV),
                ("Wait 1 h (W)", KeyCode::KeyW),
                ("Home (Z)", KeyCode::KeyZ),
            ];
            for (label, k) in acts {
                button(p, label, Act::Key(k), BTN, 13.0);
            }
        });
    });
    panel(p, at(None, Some(12.0), Some(64.0), None, Some(420.0)), "Log", |p| {
        for l in prog.log.iter().rev().take(14).rev() {
            text(p, l, 13.0, Color::WHITE);
        }
    });
    note(p, g, "Aboard a station or ship you look out of its window; right-drag to look around.");
}

// ---- flight ----

fn flight(p: &mut ChildSpawnerCommands, f: &Flight) {
    panel(p, at(Some(12.0), None, Some(64.0), None, Some(270.0)), "Mission: reach orbit", |p| {
        for (i, label) in ["Liftoff", "Pass 30 km", "Reach space (100 km)", "Stable orbit"].iter().enumerate() {
            row(p, |p| {
                live_text(p, Live::Check(i), 16.0, Color::WHITE);
                text(p, *label, 16.0, Color::WHITE);
                p.spawn(Node { flex_grow: 1.0, ..default() });
            });
        }
    });
    panel(p, at(None, Some(12.0), Some(64.0), None, Some(230.0)), "", |p| {
        let big = |p: &mut ChildSpawnerCommands, label: &str, l: Live| {
            text(p, label, 14.0, DIM);
            live_text(p, l, 24.0, Color::WHITE);
        };
        big(p, "Flight time", Live::Time);
        big(p, "Altitude", Live::Alt);
        big(p, "Speed (surface)", Live::Speed);
        big(p, "Vertical speed", Live::VSpeed);
        big(p, "Apoapsis", Live::Apo);
        big(p, "Periapsis", Live::Peri);
    });
    panel(p, at(Some(12.0), None, None, Some(16.0), Some(300.0)), "", |p| {
        let line = |p: &mut ChildSpawnerCommands, label: &str, l: Live, b: Option<(Live, Color)>| {
            row(p, |p| {
                text(p, label, 15.0, DIM);
                live_text(p, l, 15.0, Color::WHITE);
            });
            if let Some((bl, c)) = b {
                bar(p, bl, c);
            }
        };
        line(p, "Throttle", Live::Throttle, Some((Live::ThrottleBar, ACCENT)));
        line(p, "Stage propellant", Live::Fuel, Some((Live::FuelBar, GREEN)));
        line(p, "Dynamic pressure", Live::Q, Some((Live::QBar, RED)));
        line(p, "Thrust / weight", Live::Twr, None);
        line(p, "Delta-v left", Live::Dv, None);
        line(p, "Mass", Live::Mass, None);
    });
    panel(p, at(None, Some(12.0), None, Some(16.0), Some(330.0)), "Ascent profile", |p| {
        p.spawn((Node { height: px(90), align_items: AlignItems::FlexEnd, column_gap: px(1), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.04)))).with_children(|p| {
            for i in 0..60 {
                p.spawn((Node { width: px(4), height: percent(0), ..default() }, BackgroundColor(GREEN), Live::Profile(i)));
            }
        });
        live_text(p, Live::Warp, 13.0, DIM);
    });
    p.spawn((Node { position_type: PositionType::Absolute, left: px(330), right: px(360), bottom: px(16), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: px(4), ..default() }, Pickable::IGNORE))
        .with_children(|p| {
            live_text(p, Live::Event, 16.0, AMBER);
            text(p, "Z/X throttle  F full  C cut  A/D pitch  Space stage  P chute  T warp  V outside view  Esc end", 13.0, DIM);
            text(p, "In the cockpit: drag the throttle lever and stick, lift the red cover to stage, flip the chute switch. Right-drag looks around, Home recentres.", 13.0, DIM);
        });
    let _ = f;
}

fn fmt_alt(m: f64) -> String {
    if m.abs() >= 10_000.0 { format!("{:.1} km", m / 1e3) } else { format!("{m:.0} m") }
}

fn live(g: Res<Game>, mut texts: Query<(&Live, &mut Text, Option<&mut TextColor>)>, mut nodes: Query<(&Live, &mut Node), Without<Text>>) {
    let Mode::Flying(f) = &g.mode else { return };
    let el = f.elements();
    let up = f.pos.normalize();
    let computer = f.alt() > 20_000.0;
    let strength = f.vessel.weakest().map_or(80_000.0, |(_, q)| q);
    let cap = f.vessel.stage_prop(f.stage).max(1.0);
    let (t_kn, _, isp_vac) = f.vessel.stage_engines(f.stage);
    let twr = t_kn * 1000.0 * f.throttle.max(1e-9) / (f.mass() * f.local_g());
    let stats = f.vessel.stats();
    let m = f.mass();
    let dv_now = if isp_vac > 0.0 && f.prop > 0.0 { isp_vac * orbital_client::G0 * (m / (m - f.prop)).ln() } else { 0.0 };
    let dv = dv_now + stats.iter().skip(f.stage + 1).map(|s| s.dv_vac).sum::<f64>();
    let checks = [f.status != Status::OnPad, f.max_alt > 30_000.0, f.reached_space, f.reached_orbit];
    for (l, mut t, color) in &mut texts {
        t.0 = match *l {
            Live::Time => format!("{:02}:{:02}:{:02}", (f.t / 3600.0) as u64, (f.t / 60.0) as u64 % 60, f.t as u64 % 60),
            Live::Alt => fmt_alt(f.alt()),
            Live::Speed => format!("{:.0} m/s", f.rel_vel().length()),
            Live::VSpeed => format!("{:+.0} m/s", f.vel.dot(up)),
            Live::Apo => if computer { fmt_alt(el.apoapsis_r - R_EARTH) } else { "--".into() },
            Live::Peri => if computer && el.periapsis_r > 0.0 { fmt_alt(el.periapsis_r - R_EARTH) } else { "--".into() },
            Live::Twr => if f.throttle > 0.0 { format!("{twr:.2}") } else { "0".into() },
            Live::Dv => format!("{dv:.0} m/s"),
            Live::Fuel => format!("{:.0}%", f.prop / cap * 100.0),
            Live::Mass => format!("{:.1} t", m / 1e3),
            Live::Q => format!("{:.1} kPa", f.q() / 1e3),
            Live::Throttle => format!("{:.0}%", f.throttle * 100.0),
            Live::Event => f.events.last().map(|e| format!("T+{:.0} s  {}", e.t, e.msg)).unwrap_or_default(),
            Live::Check(i) => {
                if let Some(mut c) = color {
                    c.0 = if checks[i] { GREEN } else { DIM };
                }
                if checks[i] { "[x]".into() } else { "[ ]".into() }
            }
            Live::Warp => {
                let stage = format!("Stage {} of {}", f.stage + 1, f.vessel.stages.len());
                if f.alt() > SPACE_ALT * 0.75 && f.throttle == 0.0 { format!("{stage}   coasting: T to warp") } else { stage }
            }
            _ => continue,
        };
    }
    let peak = f.max_alt.max(1.0);
    let n = f.samples.len();
    for (l, mut node) in &mut nodes {
        let v = match *l {
            Live::FuelBar => f.prop / cap,
            Live::QBar => f.q() / strength,
            Live::ThrottleBar => f.throttle,
            Live::Profile(i) => {
                if n == 0 { 0.0 } else { f.samples[(i * n / 60).min(n - 1)].alt / peak }
            }
            _ => continue,
        };
        let pct = (v.clamp(0.0, 1.0) * 100.0) as f32;
        if matches!(l, Live::Profile(_)) {
            node.height = percent(pct);
        } else {
            node.width = percent(pct);
        }
    }
}

// ---- report ----

fn report(p: &mut ChildSpawnerCommands, r: &FlightReport, lines: &[String]) {
    panel(p, at(Some(60.0), Some(60.0), Some(80.0), None, None), "Flight Report", |p| {
        text(p, &r.headline, 22.0, Color::WHITE);
        row(p, |p| {
            for (k, v) in [
                ("Peak", fmt_alt(r.max_alt)),
                ("Max speed", format!("{:.0} m/s", r.max_speed)),
                ("Max-Q", format!("{:.1} kPa", r.max_q / 1e3)),
                ("Delta-v used", format!("{:.0} of {:.0} m/s", r.dv_used, r.dv_design)),
                ("Duration", format!("{:.0} s", r.duration)),
            ] {
                p.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|p| {
                    text(p, k, 13.0, DIM);
                    text(p, v, 17.0, Color::WHITE);
                });
            }
        });
        text(p, "What happened and why", 17.0, Color::WHITE);
        for d in &r.diagnosis {
            text(p, format!("- {d}"), 15.0, Color::WHITE);
        }
        text(p, "Timeline", 17.0, Color::WHITE);
        for t in r.timeline.iter().take(12) {
            text(p, t, 13.0, DIM);
        }
        for l in lines {
            text(p, l, 15.0, GREEN);
        }
        button(p, "Back to the builder (Enter)", Act::Key(KeyCode::Enter), ACCENT, 16.0);
    });
}

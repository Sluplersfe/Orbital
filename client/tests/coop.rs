//! Cross-language co-op check against the real Go server binary. Skipped unless the binary has
//! been built: `cd server && go build -o bin/orbital-server ./cmd/orbital-server`.
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use bevy::math::DVec3;
use orbital_client::net::{pb, v3, NetClient, World};
use orbital_client::orbit::circular_speed;
use orbital_client::{MU, R_EARTH};

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn wait(c: &NetClient, w: &mut World, what: &str, mut ok: impl FnMut(&World) -> bool) {
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end {
        for ev in c.poll() {
            w.apply(ev);
        }
        if ok(w) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn rust_client_against_go_server() {
    let bin = concat!(env!("CARGO_MANIFEST_DIR"), "/../server/bin/orbital-server");
    if !std::path::Path::new(bin).exists() {
        eprintln!("skipping: {bin} not built");
        return;
    }
    let dir = std::env::temp_dir().join(format!("orbital-coop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let tokens = dir.join("tokens.txt");
    let issue = |name: &str| {
        let out = Command::new(bin).args(["token", name, "-tokens"]).arg(&tokens).output().unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    let (ta, tb) = (issue("ann"), issue("bob"));
    let (tcp, udp) = ("127.0.0.1:40404", "127.0.0.1:40405");
    let _srv = Kill(Command::new(bin).args(["-tcp", tcp, "-udp", udp, "-tokens"]).arg(&tokens).spawn().unwrap());

    let connect = |tok: &str| {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            match NetClient::connect(tcp, udp, tok) {
                Ok(c) => return c,
                Err(e) if Instant::now() > end => panic!("connect: {e}"),
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    };
    let a = connect(&ta);
    assert!(NetClient::connect(tcp, udp, "forged").is_err(), "forged token accepted");
    let b = connect(&tb);
    let (mut wa, mut wb) = (World::new(&a.ack), World::new(&b.ack));
    assert!(wa.is_host());

    a.claim(0, pb::CrewRole::Pilot).unwrap();
    wait(&a, &mut wa, "own ship", |w| w.my_ship.is_some());
    let id = wa.my_ship.unwrap();

    let r = R_EARTH + 250_000.0;
    let state = pb::ShipState {
        ship_id: id, pilot: String::new(), world_time: wa.world_time(),
        position: Some(v3(DVec3::new(r, 0.0, 0.0))), velocity: Some(v3(DVec3::new(0.0, circular_speed(MU, r), 0.0))),
    };
    wait(&b, &mut wb, "relayed state", |w| {
        let _ = a.send_state(state.clone());
        w.peers.get(&id).is_some_and(|s| s.pilot == "ann")
    });

    // Shared research: the same milestone from both players is paid once, and points are spent once.
    a.award("orbit", 100).unwrap();
    b.award("orbit", 100).unwrap();
    wait(&b, &mut wb, "shared award", |w| w.program.milestones.len() == 1);
    wait(&a, &mut wa, "shared award on a", |w| w.program.rp == 100);
    b.unlock("vector", 40).unwrap();
    wait(&a, &mut wa, "shared unlock", |w| w.program.unlocked == ["vector"]);
    assert_eq!(wa.program.rp, 60);
    let mut prog = orbital_client::progress::Progress::new();
    prog.apply_shared(wa.program.rp, &wa.program.unlocked, &wa.program.milestones, &wa.program.claims);
    assert!(prog.has("vector") && prog.rp == 60);

    drop(a); // host leaves abruptly; its orbiting ship goes on rails
    wait(&b, &mut wb, "handoff", |w| w.is_host() && w.roster.ships.iter().any(|s| s.rails.is_some()));
    let others = wb.others();
    assert_eq!(others.len(), 1);
    assert!((others[0].1.length() - r).abs() < 1.0, "rails ship off its orbit: {:?}", others[0]);
}

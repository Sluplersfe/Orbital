//! Co-op networking: TCP lobby (length-prefixed Envelopes) and UDP ship state, on std threads and
//! channels so nothing blocks a Bevy system. `World` folds server events into a view of the game.
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bevy::math::DVec3;
use prost::Message;

use crate::orbit;

pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/orbital.rs"));
}
use pb::envelope::Msg;

pub enum NetEvent {
    Msg(Msg),
    Ship(pb::ShipState),
    Closed,
}

pub struct NetClient {
    tcp: Arc<Mutex<TcpStream>>,
    udp: UdpSocket,
    key: Vec<u8>,
    pub ack: pb::JoinAck,
    rx: Mutex<Receiver<NetEvent>>,
}

fn write_frame(w: &mut impl Write, msg: Msg) -> io::Result<()> {
    let body = pb::Envelope { msg: Some(msg) }.encode_to_vec();
    let mut buf = (body.len() as u32).to_be_bytes().to_vec();
    buf.extend(body);
    w.write_all(&buf)
}

fn read_frame(r: &mut impl Read) -> io::Result<Option<Msg>> {
    let mut hdr = [0u8; 4];
    r.read_exact(&mut hdr)?;
    let n = u32::from_be_bytes(hdr) as usize;
    if n > 1 << 20 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut body = vec![0u8; n];
    r.read_exact(&mut body)?;
    let env = pb::Envelope::decode(body.as_slice()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(env.msg)
}

impl NetClient {
    /// Join with a token. `udp_addr` is the server's UDP state port (normally 4005).
    pub fn connect(tcp_addr: &str, udp_addr: &str, token: &str) -> io::Result<Self> {
        let addr = tcp_addr.to_socket_addrs()?.next().ok_or(io::ErrorKind::AddrNotAvailable)?;
        let mut tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
        tcp.set_nodelay(true)?;
        write_frame(&mut tcp, Msg::Join(pb::Join { token: token.into() }))?;
        let ack = match read_frame(&mut tcp)? {
            Some(Msg::JoinAck(a)) if a.ok => a,
            Some(Msg::JoinAck(a)) => return Err(io::Error::new(io::ErrorKind::PermissionDenied, a.error)),
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "expected join ack")),
        };
        let udp = UdpSocket::bind("0.0.0.0:0")?;
        udp.connect(udp_addr)?;
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Mutex::new(tcp.try_clone()?));

        let (mut reader, writer, tx1) = (tcp, shared.clone(), tx.clone());
        thread::spawn(move || {
            while let Ok(m) = read_frame(&mut reader) {
                match m {
                    Some(Msg::Ping(p)) => {
                        let _ = write_frame(&mut *writer.lock().unwrap(), Msg::Pong(pb::Pong { nonce: p.nonce }));
                        let _ = tx1.send(NetEvent::Msg(Msg::Ping(p)));
                    }
                    Some(m) => {
                        if tx1.send(NetEvent::Msg(m)).is_err() {
                            return;
                        }
                    }
                    None => {}
                }
            }
            let _ = tx1.send(NetEvent::Closed);
        });

        let udp_rx = udp.try_clone()?;
        thread::spawn(move || {
            let mut buf = [0u8; 2048];
            while let Ok(n) = udp_rx.recv(&mut buf) {
                if let Ok(d) = pb::Datagram::decode(&buf[..n]) {
                    if let Some(s) = d.ship_state {
                        if tx.send(NetEvent::Ship(s)).is_err() {
                            return;
                        }
                    }
                }
            }
        });

        let c = Self { tcp: shared, udp, key: ack.udp_key.clone(), ack, rx: Mutex::new(rx) };
        c.send_datagram(None)?; // register our UDP address
        Ok(c)
    }

    pub fn send(&self, msg: Msg) -> io::Result<()> {
        write_frame(&mut *self.tcp.lock().unwrap(), msg)
    }

    pub fn claim(&self, ship_id: u64, role: pb::CrewRole) -> io::Result<()> {
        self.send(Msg::SeatClaim(pb::SeatClaim { ship_id, role: role as i32 }))
    }

    /// Report a first-time award; the server pays it once to the whole group.
    pub fn award(&self, key: &str, points: u32) -> io::Result<()> {
        self.program_event(pb::program_event::Event::Award(pb::ProgramAward { key: key.into(), points }))
    }

    /// Ask to buy a part with the group's research points.
    pub fn unlock(&self, part: &str, cost: u32) -> io::Result<()> {
        self.program_event(pb::program_event::Event::Unlock(pb::ProgramUnlock { part: part.into(), cost }))
    }

    pub fn claim_resource(&self, resource: &str) -> io::Result<()> {
        self.program_event(pb::program_event::Event::Claim(pb::ProgramClaim { resource: resource.into() }))
    }

    fn program_event(&self, event: pb::program_event::Event) -> io::Result<()> {
        self.send(Msg::ProgramEvent(pb::ProgramEvent { event: Some(event) }))
    }

    pub fn send_state(&self, s: pb::ShipState) -> io::Result<()> {
        self.send_datagram(Some(s))
    }

    fn send_datagram(&self, ship_state: Option<pb::ShipState>) -> io::Result<()> {
        self.udp.send(&pb::Datagram { key: self.key.clone(), ship_state }.encode_to_vec()).map(|_| ())
    }

    pub fn poll(&self) -> Vec<NetEvent> {
        self.rx.lock().unwrap().try_iter().collect()
    }
}

impl Drop for NetClient {
    fn drop(&mut self) {
        // The reader thread holds a cloned fd, so close the connection explicitly.
        if let Ok(t) = self.tcp.lock() {
            let _ = t.shutdown(std::net::Shutdown::Both);
        }
    }
}

pub fn v3(v: DVec3) -> pb::Vec3d {
    pb::Vec3d { x: v.x, y: v.y, z: v.z }
}

pub fn dv(v: &Option<pb::Vec3d>) -> DVec3 {
    v.as_ref().map_or(DVec3::ZERO, |v| DVec3::new(v.x, v.y, v.z))
}

/// Position of an on-rails ship at world time t: a pure function of its rails state.
pub fn rail_position(r: &pb::RailShip, t: f64) -> DVec3 {
    orbit::propagate(crate::MU, dv(&r.position), dv(&r.velocity), t - r.epoch).0
}

/// What this client knows of the shared world.
#[derive(Default)]
pub struct World {
    pub me: String,
    pub host: String,
    pub roster: pb::Roster,
    pub peers: HashMap<u64, pb::ShipState>,
    pub my_ship: Option<u64>,
    pub error: String,
    pub closed: bool,
    /// The group's shared research, as last told by the server.
    pub program: pb::ProgramState,
    /// Short lines about what the group just did ("bob unlocked hab1").
    pub notes: Vec<String>,
    clock: f64,
    clock_at: Option<Instant>,
}

impl World {
    pub fn new(ack: &pb::JoinAck) -> Self {
        let mut w = Self { me: ack.player.clone(), host: ack.host.clone(), program: ack.program.clone().unwrap_or_default(), ..Default::default() };
        w.sync_clock(ack.world_time);
        w
    }

    fn sync_clock(&mut self, t: f64) {
        self.clock = t;
        self.clock_at = Some(Instant::now());
    }

    /// World time in seconds, extrapolated from the last server sync.
    pub fn world_time(&self) -> f64 {
        self.clock + self.clock_at.map_or(0.0, |at| at.elapsed().as_secs_f64())
    }

    pub fn is_host(&self) -> bool {
        self.host == self.me
    }

    pub fn apply(&mut self, ev: NetEvent) {
        match ev {
            NetEvent::Ship(s) => {
                self.peers.insert(s.ship_id, s);
            }
            NetEvent::Closed => self.closed = true,
            NetEvent::Msg(m) => match m {
                Msg::Ping(p) => self.sync_clock(p.world_time),
                Msg::HostChange(h) => self.host = h.new_host,
                Msg::Error(e) => self.error = e.text,
                Msg::ProgramUpdate(u) => {
                    if let Some(st) = u.state {
                        if st.version >= self.program.version {
                            self.program = st;
                        }
                    }
                    if !u.note.is_empty() {
                        self.notes.push(u.note);
                    }
                }
                Msg::Roster(r) => {
                    self.host = r.host.clone();
                    self.sync_clock(r.world_time);
                    let me = self.me.clone();
                    self.my_ship = r.ships.iter().find(|s| {
                        s.seats.iter().any(|x| x.player == me && x.role == pb::CrewRole::Pilot as i32)
                    }).map(|s| s.ship_id);
                    // Drop relayed state for ships that are no longer piloted.
                    self.peers.retain(|id, _| r.ships.iter().any(|s| s.ship_id == *id && s.rails.is_none() && !s.frozen));
                    self.roster = r;
                }
                _ => {}
            },
        }
    }

    /// Positions of every other ship: relayed pilot state, rails, or frozen last state.
    pub fn others(&self) -> Vec<(u64, DVec3)> {
        let t = self.world_time();
        self.roster.ships.iter().filter(|s| Some(s.ship_id) != self.my_ship).filter_map(|s| {
            if let Some(r) = &s.rails {
                Some((s.ship_id, rail_position(r, t)))
            } else if let Some(p) = self.peers.get(&s.ship_id) {
                Some((s.ship_id, dv(&p.position)))
            } else {
                s.last.as_ref().map(|l| (s.ship_id, dv(&l.position)))
            }
        }).collect()
    }
}

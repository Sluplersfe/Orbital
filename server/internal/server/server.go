// Package server is the Orbital relay: the TCP lobby (join, seats, host election and handoff) and
// the UDP relay for pilot-authoritative ship state.
package server

import (
	"crypto/rand"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"log"
	"math"
	"net"
	"sort"
	"sync"
	"time"

	"google.golang.org/protobuf/proto"

	"orbital/server/internal/orbit"
	"orbital/server/internal/pb"
	"orbital/server/internal/session"
)

const (
	maxFrame   = 1 << 20
	outboxSize = 256
	atmoTop    = 70_000.0 // matches client ATMO_TOP
	healthWin  = 20       // pings in the loss window
)

type Config struct {
	TCPAddr, UDPAddr string
	PingInterval     time.Duration
	Session          *session.Session
}

type Server struct {
	cfg  Config
	sess *session.Session
	tcp  net.Listener
	udp  net.PacketConn
	done chan struct{}
	wg   sync.WaitGroup

	mu       sync.Mutex
	clients  map[string]*client
	keys     map[string]string // udp key -> player
	ships    map[uint64]*ship
	nextShip uint64
	host     string
	stations []byte // opaque station state from the latest host upload
}

type client struct {
	name    string
	conn    net.Conn
	out     chan *pb.Envelope
	key     []byte
	udp     net.Addr
	nonce   uint64
	pending map[uint64]time.Time
	rtt     time.Duration
	results []bool // last ping outcomes, true = answered
}

type ship struct {
	id     uint64
	seats  map[string]pb.CrewRole
	last   *pb.ShipState
	lastAt float64 // world time when last arrived
	rails  *pb.RailShip
	frozen bool
}

// Listen binds both ports. Call Serve to start handling traffic.
func Listen(cfg Config) (*Server, error) {
	if cfg.PingInterval == 0 {
		cfg.PingInterval = time.Second
	}
	if cfg.Session == nil {
		cfg.Session = session.New()
	}
	tcp, err := net.Listen("tcp", cfg.TCPAddr)
	if err != nil {
		return nil, err
	}
	udp, err := net.ListenPacket("udp", cfg.UDPAddr)
	if err != nil {
		tcp.Close()
		return nil, err
	}
	return &Server{
		cfg: cfg, sess: cfg.Session, tcp: tcp, udp: udp, done: make(chan struct{}),
		clients: map[string]*client{}, keys: map[string]string{}, ships: map[uint64]*ship{},
	}, nil
}

func (s *Server) TCPAddr() net.Addr { return s.tcp.Addr() }
func (s *Server) UDPAddr() net.Addr { return s.udp.LocalAddr() }
func (s *Server) Clock() float64    { return s.sess.Clock() }

func (s *Server) Serve() {
	s.wg.Add(3)
	go s.acceptLoop()
	go s.udpLoop()
	go s.pingLoop()
}

func (s *Server) Close() {
	close(s.done)
	s.tcp.Close()
	s.udp.Close()
	s.mu.Lock()
	for _, c := range s.clients {
		c.conn.Close()
	}
	s.mu.Unlock()
	s.wg.Wait()
}

func (s *Server) acceptLoop() {
	defer s.wg.Done()
	for {
		c, err := s.tcp.Accept()
		if err != nil {
			return
		}
		go s.handleConn(c)
	}
}

func (s *Server) handleConn(c net.Conn) {
	defer c.Close()
	c.SetReadDeadline(time.Now().Add(10 * time.Second))
	env, err := readFrame(c)
	if err != nil {
		return
	}
	join := env.GetJoin()
	if join == nil {
		writeFrame(c, &pb.Envelope{Msg: &pb.Envelope_JoinAck{JoinAck: &pb.JoinAck{Error: "expected join"}}})
		return
	}
	p, err := s.sess.Join(join.Token)
	if err != nil {
		writeFrame(c, &pb.Envelope{Msg: &pb.Envelope_JoinAck{JoinAck: &pb.JoinAck{Error: err.Error()}}})
		return
	}
	c.SetReadDeadline(time.Time{})
	cl := &client{name: p.Name, conn: c, out: make(chan *pb.Envelope, outboxSize), key: randKey(), pending: map[uint64]time.Time{}}
	go writer(cl)

	s.mu.Lock()
	host, _ := s.sess.Host()
	ack := &pb.JoinAck{Ok: true, Player: p.Name, Host: host, WorldTime: s.sess.Clock(), UdpKey: cl.key}
	if host == p.Name {
		ack.Snapshot = s.snapshot()
	}
	send(cl, &pb.Envelope{Msg: &pb.Envelope_JoinAck{JoinAck: ack}})
	s.clients[p.Name] = cl
	s.keys[string(cl.key)] = p.Name
	s.checkHost()
	s.broadcastRoster()
	s.mu.Unlock()
	log.Printf("join %s (host %s)", p.Name, host)

	for {
		// Live clients answer every ping, so silence this long means the link is dead
		// (no FIN after Wi-Fi loss or sleep). Dropping them unfreezes rails and the clock.
		c.SetReadDeadline(time.Now().Add(10 * s.cfg.PingInterval))
		env, err := readFrame(c)
		if err != nil {
			break
		}
		s.mu.Lock()
		if e := s.handle(cl, env); e != nil {
			send(cl, &pb.Envelope{Msg: &pb.Envelope_Error{Error: &pb.Error{Text: e.Error()}}})
		}
		s.mu.Unlock()
	}

	s.mu.Lock()
	delete(s.clients, cl.name)
	delete(s.keys, string(cl.key))
	close(cl.out)
	for id, sh := range s.ships {
		if _, ok := sh.seats[cl.name]; ok {
			s.releaseSeat(cl.name, id)
		}
	}
	s.sess.Leave(cl.name)
	s.checkHost()
	s.broadcastRoster()
	s.mu.Unlock()
	log.Printf("leave %s", cl.name)
}

// handle runs one TCP message from a joined player. Caller holds s.mu.
func (s *Server) handle(cl *client, env *pb.Envelope) error {
	switch m := env.Msg.(type) {
	case *pb.Envelope_Pong:
		if t, ok := cl.pending[m.Pong.Nonce]; ok {
			delete(cl.pending, m.Pong.Nonce)
			rtt := time.Since(t)
			if cl.rtt == 0 {
				cl.rtt = rtt
			} else {
				cl.rtt = (cl.rtt*7 + rtt) / 8
			}
			cl.record(true)
			s.sess.Report(cl.name, cl.rtt, cl.loss())
		}
	case *pb.Envelope_SeatClaim:
		if err := s.claimSeat(cl.name, m.SeatClaim.ShipId, m.SeatClaim.Role); err != nil {
			return err
		}
		s.broadcastRoster()
	case *pb.Envelope_SeatRelease:
		sh := s.ships[m.SeatRelease.ShipId]
		if sh == nil {
			return errors.New("no such ship")
		}
		if _, ok := sh.seats[cl.name]; !ok {
			return errors.New("not seated on that ship")
		}
		s.releaseSeat(cl.name, sh.id)
		// An explicit release ends the flight: a ship that is not orbiting is done with.
		if sh.frozen && len(sh.seats) == 0 {
			delete(s.ships, sh.id)
		}
		s.broadcastRoster()
	case *pb.Envelope_HostSnapshot:
		if cl.name != s.host {
			return errors.New("only the host uploads snapshots")
		}
		s.stations = m.HostSnapshot.Stations
	default:
		return fmt.Errorf("unexpected message %T", env.Msg)
	}
	return nil
}

func (s *Server) claimSeat(name string, id uint64, role pb.CrewRole) error {
	if role == pb.CrewRole_CREW_ROLE_UNSPECIFIED {
		return errors.New("no role given")
	}
	if id == 0 {
		if role != pb.CrewRole_CREW_ROLE_PILOT {
			return errors.New("a new ship needs a pilot")
		}
		s.releaseAll(name)
		s.nextShip++
		s.ships[s.nextShip] = &ship{id: s.nextShip, seats: map[string]pb.CrewRole{name: role}}
		return nil
	}
	sh := s.ships[id]
	if sh == nil {
		return errors.New("no such ship")
	}
	for p, r := range sh.seats {
		if r == role && p != name {
			return fmt.Errorf("%s seat taken by %s", role, p)
		}
	}
	for oid, o := range s.ships {
		if _, ok := o.seats[name]; ok && oid != id {
			s.releaseSeat(name, oid)
		}
	}
	old := sh.seats[name]
	sh.seats[name] = role
	if old == pb.CrewRole_CREW_ROLE_PILOT && role != old {
		s.unpilot(sh)
	}
	return nil
}

func (s *Server) releaseAll(name string) {
	for id, sh := range s.ships {
		if _, ok := sh.seats[name]; ok {
			s.releaseSeat(name, id)
		}
	}
}

// releaseSeat removes a player's seat. A ship that loses its pilot goes on rails if it is in a
// stable orbit, and is frozen in place otherwise.
func (s *Server) releaseSeat(name string, id uint64) {
	sh := s.ships[id]
	role := sh.seats[name]
	delete(sh.seats, name)
	if role == pb.CrewRole_CREW_ROLE_PILOT {
		s.unpilot(sh)
	}
	if len(sh.seats) == 0 && sh.last == nil {
		delete(s.ships, id)
	}
}

func (s *Server) unpilot(sh *ship) {
	if sh.last == nil {
		return
	}
	r, v := vec(sh.last.Position), vec(sh.last.Velocity)
	el := orbit.FromState(orbit.MU, r, v)
	if el.Energy < 0 && el.PeriapsisR > orbit.REarth+atmoTop {
		sh.rails = &pb.RailShip{ShipId: sh.id, Position: sh.last.Position, Velocity: sh.last.Velocity, Epoch: sh.lastAt}
		sh.frozen = false
	} else {
		sh.rails, sh.frozen = nil, true
	}
}

// checkHost broadcasts a HostChange when the elected host differs from the current one.
func (s *Server) checkHost() {
	h, _ := s.sess.Host()
	if h == s.host {
		return
	}
	s.host = h
	if h == "" {
		return
	}
	log.Printf("host is now %s", h)
	env := &pb.Envelope{Msg: &pb.Envelope_HostChange{HostChange: &pb.HostChange{NewHost: h, Snapshot: s.snapshot()}}}
	for _, c := range s.clients {
		send(c, env)
	}
}

// snapshot is the host-owned world state. Rails come from the server's ship table, so a host
// that dies without uploading loses nothing but unsynced station state.
func (s *Server) snapshot() *pb.HostSnapshot {
	snap := &pb.HostSnapshot{WorldTime: s.sess.Clock(), Stations: s.stations}
	for _, sh := range s.sortedShips() {
		if sh.rails != nil {
			snap.Rails = append(snap.Rails, sh.rails)
		}
	}
	return snap
}

func (s *Server) sortedShips() []*ship {
	out := make([]*ship, 0, len(s.ships))
	for _, sh := range s.ships {
		out = append(out, sh)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].id < out[j].id })
	return out
}

func (s *Server) broadcastRoster() {
	r := &pb.Roster{Host: s.host, WorldTime: s.sess.Clock()}
	for n := range s.clients {
		r.Players = append(r.Players, n)
	}
	sort.Strings(r.Players)
	for _, sh := range s.sortedShips() {
		m := &pb.Ship{ShipId: sh.id, Rails: sh.rails, Frozen: sh.frozen, Last: sh.last}
		for p, role := range sh.seats {
			m.Seats = append(m.Seats, &pb.Seat{Player: p, Role: role})
		}
		sort.Slice(m.Seats, func(i, j int) bool { return m.Seats[i].Role < m.Seats[j].Role })
		r.Ships = append(r.Ships, m)
	}
	env := &pb.Envelope{Msg: &pb.Envelope_Roster{Roster: r}}
	for _, c := range s.clients {
		send(c, env)
	}
}

func (s *Server) udpLoop() {
	defer s.wg.Done()
	buf := make([]byte, 2048)
	for {
		n, addr, err := s.udp.ReadFrom(buf)
		if err != nil {
			select {
			case <-s.done:
				return
			default:
				continue
			}
		}
		var d pb.Datagram
		if proto.Unmarshal(buf[:n], &d) != nil {
			continue
		}
		s.mu.Lock()
		name, ok := s.keys[string(d.Key)]
		if !ok {
			s.mu.Unlock()
			continue
		}
		s.clients[name].udp = addr
		ss := d.ShipState
		if ss == nil || !s.acceptState(name, ss) {
			s.mu.Unlock()
			continue
		}
		var peers []net.Addr
		for n, c := range s.clients {
			if n != name && c.udp != nil {
				peers = append(peers, c.udp)
			}
		}
		s.mu.Unlock()
		out, _ := proto.Marshal(&pb.Datagram{ShipState: ss})
		for _, a := range peers {
			s.udp.WriteTo(out, a)
		}
	}
}

// acceptState applies basic sanity checks: only the ship's pilot may send its state, and the
// state must be finite and physically plausible.
func (s *Server) acceptState(name string, ss *pb.ShipState) bool {
	sh := s.ships[ss.ShipId]
	if sh == nil || sh.seats[name] != pb.CrewRole_CREW_ROLE_PILOT {
		return false
	}
	r, v := vec(ss.Position), vec(ss.Velocity)
	for _, x := range append(r[:], v[:]...) {
		if math.IsNaN(x) || math.IsInf(x, 0) {
			return false
		}
	}
	if r.Len() < orbit.REarth*0.99 || v.Len() > 50_000 {
		return false
	}
	ss.Pilot = name
	sh.last, sh.lastAt = ss, s.sess.Clock()
	sh.rails, sh.frozen = nil, false
	return true
}

func (s *Server) pingLoop() {
	defer s.wg.Done()
	t := time.NewTicker(s.cfg.PingInterval)
	defer t.Stop()
	for {
		select {
		case <-s.done:
			return
		case <-t.C:
		}
		s.mu.Lock()
		now := time.Now()
		for _, c := range s.clients {
			for n, at := range c.pending {
				if now.Sub(at) > 3*s.cfg.PingInterval {
					delete(c.pending, n)
					c.record(false)
				}
			}
			s.sess.Report(c.name, c.rtt, c.loss())
			c.nonce++
			c.pending[c.nonce] = now
			send(c, &pb.Envelope{Msg: &pb.Envelope_Ping{Ping: &pb.Ping{Nonce: c.nonce, WorldTime: s.sess.Clock()}}})
		}
		prev := s.host
		s.checkHost()
		if s.host != prev {
			s.broadcastRoster()
		}
		s.mu.Unlock()
	}
}

func (c *client) record(ok bool) {
	c.results = append(c.results, ok)
	if len(c.results) > healthWin {
		c.results = c.results[1:]
	}
}

func (c *client) loss() float64 {
	if len(c.results) == 0 {
		return 0
	}
	lost := 0
	for _, ok := range c.results {
		if !ok {
			lost++
		}
	}
	return float64(lost) / float64(len(c.results))
}

// send queues a message. A client whose outbox is full is too slow and is disconnected.
// Caller holds s.mu (the outbox is closed under it).
func send(c *client, env *pb.Envelope) {
	select {
	case c.out <- env:
	default:
		c.conn.Close()
	}
}

func writer(c *client) {
	for env := range c.out {
		c.conn.SetWriteDeadline(time.Now().Add(5 * time.Second))
		if writeFrame(c.conn, env) != nil {
			c.conn.Close()
		}
	}
}

func readFrame(r io.Reader) (*pb.Envelope, error) {
	var hdr [4]byte
	if _, err := io.ReadFull(r, hdr[:]); err != nil {
		return nil, err
	}
	n := binary.BigEndian.Uint32(hdr[:])
	if n > maxFrame {
		return nil, errors.New("frame too large")
	}
	buf := make([]byte, n)
	if _, err := io.ReadFull(r, buf); err != nil {
		return nil, err
	}
	var env pb.Envelope
	return &env, proto.Unmarshal(buf, &env)
}

func writeFrame(w io.Writer, env *pb.Envelope) error {
	b, err := proto.Marshal(env)
	if err != nil {
		return err
	}
	out := binary.BigEndian.AppendUint32(make([]byte, 0, 4+len(b)), uint32(len(b)))
	_, err = w.Write(append(out, b...))
	return err
}

func vec(v *pb.Vec3D) orbit.Vec3 {
	if v == nil {
		return orbit.Vec3{}
	}
	return orbit.Vec3{v.X, v.Y, v.Z}
}

func randKey() []byte {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		panic(err)
	}
	return b
}

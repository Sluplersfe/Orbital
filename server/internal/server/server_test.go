package server

import (
	"net"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"

	"orbital/server/internal/orbit"
	"orbital/server/internal/pb"
	"orbital/server/internal/session"
)

type tclient struct {
	t    *testing.T
	name string
	tcp  net.Conn
	udp  net.PacketConn
	srv  net.Addr
	key  []byte
	env  chan *pb.Envelope
	dg   chan *pb.ShipState
}

func dial(t *testing.T, s *Server, token string) *tclient {
	t.Helper()
	c, err := net.Dial("tcp", s.TCPAddr().String())
	if err != nil {
		t.Fatal(err)
	}
	writeFrame(c, &pb.Envelope{Msg: &pb.Envelope_Join{Join: &pb.Join{Token: token}}})
	env, err := readFrame(c)
	if err != nil || !env.GetJoinAck().GetOk() {
		t.Fatalf("join: %v %v", err, env)
	}
	u, _ := net.ListenPacket("udp", "127.0.0.1:0")
	tc := &tclient{t: t, name: env.GetJoinAck().Player, tcp: c, udp: u, srv: s.UDPAddr(), key: env.GetJoinAck().UdpKey,
		env: make(chan *pb.Envelope, 1024), dg: make(chan *pb.ShipState, 1024)}
	tc.env <- env
	go func() {
		for {
			e, err := readFrame(c)
			if err != nil {
				return
			}
			if p := e.GetPing(); p != nil {
				writeFrame(c, &pb.Envelope{Msg: &pb.Envelope_Pong{Pong: &pb.Pong{Nonce: p.Nonce}}})
				continue
			}
			tc.env <- e
		}
	}()
	go func() {
		buf := make([]byte, 2048)
		for {
			n, _, err := u.ReadFrom(buf)
			if err != nil {
				return
			}
			var d pb.Datagram
			if proto.Unmarshal(buf[:n], &d) == nil && d.ShipState != nil {
				tc.dg <- d.ShipState
			}
		}
	}()
	tc.sendUDP(nil)
	return tc
}

func (c *tclient) send(env *pb.Envelope) { writeFrame(c.tcp, env) }

func (c *tclient) sendUDP(ss *pb.ShipState) {
	b, _ := proto.Marshal(&pb.Datagram{Key: c.key, ShipState: ss})
	c.udp.WriteTo(b, c.srv)
}

// await returns the first message matching ok, failing after a timeout.
func (c *tclient) await(what string, ok func(*pb.Envelope) bool) *pb.Envelope {
	c.t.Helper()
	deadline := time.After(2 * time.Second)
	for {
		select {
		case e := <-c.env:
			if ok(e) {
				return e
			}
		case <-deadline:
			c.t.Fatalf("%s: timed out waiting for %s", c.name, what)
		}
	}
}

func (c *tclient) roster(ok func(*pb.Roster) bool) *pb.Roster {
	c.t.Helper()
	return c.await("roster", func(e *pb.Envelope) bool { return e.GetRoster() != nil && ok(e.GetRoster()) }).GetRoster()
}

func claim(c *tclient, id uint64, role pb.CrewRole) {
	c.send(&pb.Envelope{Msg: &pb.Envelope_SeatClaim{SeatClaim: &pb.SeatClaim{ShipId: id, Role: role}}})
}

func orbitState(id uint64, alt float64) *pb.ShipState {
	r := orbit.REarth + alt
	return &pb.ShipState{ShipId: id, Position: &pb.Vec3D{X: r}, Velocity: &pb.Vec3D{Y: orbit.CircularSpeed(orbit.MU, r)}}
}

func shipByID(r *pb.Roster, id uint64) *pb.Ship {
	for _, s := range r.Ships {
		if s.ShipId == id {
			return s
		}
	}
	return nil
}

func TestCoopGate(t *testing.T) {
	sess := session.New()
	s, err := Listen(Config{TCPAddr: "127.0.0.1:0", UDPAddr: "127.0.0.1:0", PingInterval: 20 * time.Millisecond, Session: sess})
	if err != nil {
		t.Fatal(err)
	}
	s.Serve()
	defer s.Close()

	if c, _ := net.Dial("tcp", s.TCPAddr().String()); c != nil {
		writeFrame(c, &pb.Envelope{Msg: &pb.Envelope_Join{Join: &pb.Join{Token: "forged"}}})
		if e, _ := readFrame(c); e.GetJoinAck().GetOk() {
			t.Fatal("forged token accepted")
		}
		c.Close()
	}

	var cs []*tclient
	for _, n := range []string{"ann", "bob", "cy", "dee"} {
		cs = append(cs, dial(t, s, sess.IssueToken(n)))
		time.Sleep(5 * time.Millisecond) // distinct join order
	}
	ann, bob, cy, dee := cs[0], cs[1], cs[2], cs[3]
	dee.roster(func(r *pb.Roster) bool { return len(r.Players) == 4 && r.Host == "ann" })

	// Pilot-owned ships and crew roles.
	claim(ann, 0, pb.CrewRole_CREW_ROLE_PILOT)
	dee.roster(func(r *pb.Roster) bool { return shipByID(r, 1) != nil })
	claim(bob, 1, pb.CrewRole_CREW_ROLE_NAVIGATOR)
	dee.roster(func(r *pb.Roster) bool { return len(shipByID(r, 1).GetSeats()) == 2 })
	claim(dee, 1, pb.CrewRole_CREW_ROLE_NAVIGATOR)
	dee.await("seat-taken error", func(e *pb.Envelope) bool { return e.GetError() != nil })
	claim(dee, 1, pb.CrewRole_CREW_ROLE_FLIGHT_ENGINEER)
	claim(cy, 0, pb.CrewRole_CREW_ROLE_PILOT)
	dee.roster(func(r *pb.Roster) bool {
		return shipByID(r, 2) != nil && len(shipByID(r, 1).GetSeats()) == 3
	})

	// Pilot state relays over UDP; a non-pilot cannot write a ship's state.
	bob.sendUDP(orbitState(1, 999_000))
	ann.sendUDP(orbitState(1, 300_000))
	got := <-dee.dg
	if got.Pilot != "ann" || got.Position.X != orbit.REarth+300_000 {
		t.Fatalf("relay: %v", got)
	}
	cy.sendUDP(orbitState(2, 200_000))
	<-dee.dg

	// cy leaves the seat: an orbiting ship goes on rails.
	cy.send(&pb.Envelope{Msg: &pb.Envelope_SeatRelease{SeatRelease: &pb.SeatRelease{ShipId: 2}}})
	before := dee.roster(func(r *pb.Roster) bool { return shipByID(r, 2).GetRails() != nil })
	rails := shipByID(before, 2).Rails

	// Forced handoff: the host drops abruptly. bob is next and gets the snapshot.
	clockBefore := s.Clock()
	ann.tcp.Close()
	hc := bob.await("host change", func(e *pb.Envelope) bool { return e.GetHostChange().GetNewHost() == "bob" }).GetHostChange()
	if hc.Snapshot.WorldTime < clockBefore {
		t.Fatalf("clock went back: %v < %v", hc.Snapshot.WorldTime, clockBefore)
	}
	var after *pb.RailShip
	for _, r := range hc.Snapshot.Rails {
		if r.ShipId == 2 {
			after = r
		}
	}
	if !proto.Equal(after, rails) {
		t.Fatalf("ship 2 rails changed on handoff:\n%v\n%v", rails, after)
	}
	// ann's ship lost its pilot while orbiting: also on rails, at ann's last state.
	r := dee.roster(func(r *pb.Roster) bool { return r.Host == "bob" && len(r.Players) == 3 })
	if sh := shipByID(r, 1); sh.Rails == nil || sh.Rails.Position.X != orbit.REarth+300_000 {
		t.Fatalf("ship 1 not on rails at last state: %v", sh)
	}

	// Empty server freezes the clock.
	for _, c := range cs[1:] {
		c.tcp.Close()
	}
	deadline := time.Now().Add(2 * time.Second)
	for {
		if _, any := sess.Host(); !any {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("players never left")
		}
		time.Sleep(5 * time.Millisecond)
	}
	c0 := s.Clock()
	time.Sleep(100 * time.Millisecond)
	if c1 := s.Clock(); c1 != c0 {
		t.Fatalf("clock ran while empty: %v -> %v", c0, c1)
	}
}

func TestSilentClientDropped(t *testing.T) {
	sess := session.New()
	s, err := Listen(Config{TCPAddr: "127.0.0.1:0", UDPAddr: "127.0.0.1:0", PingInterval: 10 * time.Millisecond, Session: sess})
	if err != nil {
		t.Fatal(err)
	}
	s.Serve()
	defer s.Close()
	live := dial(t, s, sess.IssueToken("live"))
	tok := sess.IssueToken("ghost")
	c, _ := net.Dial("tcp", s.TCPAddr().String())
	defer c.Close()
	writeFrame(c, &pb.Envelope{Msg: &pb.Envelope_Join{Join: &pb.Join{Token: tok}}})
	readFrame(c)
	// ghost never reads or answers pings, as after a silent link loss.
	live.roster(func(r *pb.Roster) bool { return len(r.Players) == 2 })
	live.roster(func(r *pb.Roster) bool { return len(r.Players) == 1 })
	if _, err := sess.Join(tok); err != nil {
		t.Fatalf("ghost cannot rejoin: %v", err)
	}
}

func TestEndedFlightRemovesGroundedShip(t *testing.T) {
	sess := session.New()
	s, _ := Listen(Config{TCPAddr: "127.0.0.1:0", UDPAddr: "127.0.0.1:0", Session: sess})
	s.Serve()
	defer s.Close()
	a := dial(t, s, sess.IssueToken("ann"))
	claim(a, 0, pb.CrewRole_CREW_ROLE_PILOT)
	a.roster(func(r *pb.Roster) bool { return shipByID(r, 1) != nil })
	a.sendUDP(&pb.ShipState{ShipId: 1, Position: &pb.Vec3D{X: orbit.REarth}, Velocity: &pb.Vec3D{}})
	deadline := time.Now().Add(2 * time.Second)
	for {
		s.mu.Lock()
		ok := s.ships[1] != nil && s.ships[1].last != nil
		s.mu.Unlock()
		if ok {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("state never arrived")
		}
		a.sendUDP(&pb.ShipState{ShipId: 1, Position: &pb.Vec3D{X: orbit.REarth}, Velocity: &pb.Vec3D{}})
		time.Sleep(5 * time.Millisecond)
	}
	a.send(&pb.Envelope{Msg: &pb.Envelope_SeatRelease{SeatRelease: &pb.SeatRelease{ShipId: 1}}})
	a.roster(func(r *pb.Roster) bool { return len(r.Ships) == 0 })
}

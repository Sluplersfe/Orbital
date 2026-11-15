// Package session holds join tokens, the player roster, host election and the world clock.
package session

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"sort"
	"strings"
	"sync"
	"time"
)

var (
	ErrBadToken  = errors.New("invalid join token")
	ErrNameInUse = errors.New("name already connected")
)

// Health thresholds that make a player ineligible to be host (tunable; see PLAN.md open question).
const (
	MaxHostLatency = 150 * time.Millisecond
	MaxHostLoss    = 0.05
)

type Player struct {
	Name     string
	Joined   time.Time
	Latency  time.Duration
	LossRate float64
}

func (p *Player) Healthy() bool { return p.Latency <= MaxHostLatency && p.LossRate <= MaxHostLoss }

type Session struct {
	mu      sync.Mutex
	tokens  map[string]string // token -> player name
	players map[string]*Player
	clock   float64 // world seconds
	last    time.Time
	now     func() time.Time
}

func New() *Session { return newWithClock(time.Now) }

func newWithClock(now func() time.Time) *Session {
	return &Session{tokens: map[string]string{}, players: map[string]*Player{}, now: now}
}

// IssueToken creates a join token for a trusted player name.
func (s *Session) IssueToken(name string) string {
	tok := NewToken()
	s.AddToken(name, tok)
	return tok
}

// AddToken trusts an existing token for a player name.
func (s *Session) AddToken(name, tok string) {
	s.mu.Lock()
	s.tokens[tok] = name
	s.mu.Unlock()
}

// LoadTokens reads a tokens file: one "name token" pair per line, # comments allowed.
func (s *Session) LoadTokens(path string) (int, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return 0, err
	}
	n := 0
	for _, line := range strings.Split(string(data), "\n") {
		f := strings.Fields(line)
		if len(f) == 0 || strings.HasPrefix(f[0], "#") {
			continue
		}
		if len(f) != 2 {
			return n, fmt.Errorf("%s: bad line %q", path, line)
		}
		s.AddToken(f[0], f[1])
		n++
	}
	return n, nil
}

// NewToken returns a random join token.
func NewToken() string {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		panic(err)
	}
	return hex.EncodeToString(b)
}

// Join admits a player with a valid token. The world clock starts when the first player joins.
func (s *Session) Join(token string) (Player, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	name, ok := s.tokens[token]
	if !ok {
		return Player{}, ErrBadToken
	}
	if _, dup := s.players[name]; dup {
		return Player{}, ErrNameInUse
	}
	s.advance()
	p := &Player{Name: name, Joined: s.now()}
	s.players[name] = p
	return *p, nil
}

func (s *Session) Leave(name string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.advance()
	delete(s.players, name)
}

// Report updates a player's connection health.
func (s *Session) Report(name string, latency time.Duration, loss float64) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if p := s.players[name]; p != nil {
		p.Latency, p.LossRate = latency, loss
	}
}

// Host is the earliest-joined healthy player; if nobody is healthy, the earliest-joined player.
func (s *Session) Host() (string, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	ps := make([]*Player, 0, len(s.players))
	for _, p := range s.players {
		ps = append(ps, p)
	}
	if len(ps) == 0 {
		return "", false
	}
	sort.Slice(ps, func(i, j int) bool { return ps[i].Joined.Before(ps[j].Joined) })
	for _, p := range ps {
		if p.Healthy() {
			return p.Name, true
		}
	}
	return ps[0].Name, true
}

// advance adds elapsed real time to the clock, but only while players are connected.
// Callers hold s.mu and call it before changing the roster.
func (s *Session) advance() {
	t := s.now()
	if len(s.players) > 0 {
		s.clock += t.Sub(s.last).Seconds()
	}
	s.last = t
}

// Clock returns world time in seconds. It is frozen while the server is empty.
func (s *Session) Clock() float64 {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.advance()
	return s.clock
}

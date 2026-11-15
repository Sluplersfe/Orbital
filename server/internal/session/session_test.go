package session

import (
	"testing"
	"time"
)

type fakeTime struct{ t time.Time }

func (f *fakeTime) now() time.Time      { return f.t }
func (f *fakeTime) add(d time.Duration) { f.t = f.t.Add(d) }

func TestTokens(t *testing.T) {
	s := New()
	if _, err := s.Join("nope"); err != ErrBadToken {
		t.Fatal(err)
	}
	tok := s.IssueToken("ann")
	if _, err := s.Join(tok); err != nil {
		t.Fatal(err)
	}
	if _, err := s.Join(tok); err != ErrNameInUse {
		t.Fatal(err)
	}
}

func TestHostElectionAndHandoff(t *testing.T) {
	ft := &fakeTime{t: time.Unix(1000, 0)}
	s := newWithClock(ft.now)
	for _, n := range []string{"ann", "bob", "cy"} {
		if _, err := s.Join(s.IssueToken(n)); err != nil {
			t.Fatal(err)
		}
		ft.add(time.Second)
	}
	if h, _ := s.Host(); h != "ann" {
		t.Fatal(h)
	}
	s.Report("ann", 400*time.Millisecond, 0)
	if h, _ := s.Host(); h != "bob" {
		t.Fatalf("unhealthy host kept: %s", h)
	}
	s.Leave("bob")
	if h, _ := s.Host(); h != "cy" {
		t.Fatal(h)
	}
	s.Report("cy", 400*time.Millisecond, 0)
	if h, _ := s.Host(); h != "ann" {
		t.Fatalf("fallback to earliest: %s", h)
	}
}

func TestEmptyFreezesClock(t *testing.T) {
	ft := &fakeTime{t: time.Unix(1000, 0)}
	s := newWithClock(ft.now)
	ft.add(time.Hour)
	if c := s.Clock(); c != 0 {
		t.Fatalf("empty clock ran: %v", c)
	}
	s.Join(s.IssueToken("ann"))
	ft.add(10 * time.Second)
	s.Leave("ann")
	ft.add(time.Hour)
	if c := s.Clock(); c != 10 {
		t.Fatalf("clock %v want 10", c)
	}
}

package orbit

import (
	"math"
	"testing"
)

func TestCircular(t *testing.T) {
	r := REarth + 200_000
	el := FromState(MU, Vec3{r, 0, 0}, Vec3{0, CircularSpeed(MU, r), 0})
	if math.Abs(el.A-r) > 1e-3 || el.E > 1e-9 {
		t.Fatalf("%+v", el)
	}
	if p := el.Period(MU); math.Abs(p-5301) > 5 {
		t.Fatalf("period %v", p)
	}
}

func TestKepler(t *testing.T) {
	for _, c := range [][2]float64{{1.0, 0.1}, {3.0, 0.7}, {0.2, 0.95}} {
		ea := SolveKepler(c[0], c[1])
		if math.Abs(ea-c[1]*math.Sin(ea)-c[0]) > 1e-11 {
			t.Fatalf("%v", c)
		}
	}
	// Shared with client/src/orbit.rs.
	if math.Abs(SolveKepler(1.0, 0.5)-1.4987011335178482) > 1e-12 {
		t.Fatal("shared vector")
	}
}

// Shared with client/src/orbit.rs: r0=(7e6,0,0), v0=(0,7800,1000), dt=1234 s.
var propVecR = Vec3{1800978.2209196573, 7167400.1039350107, 918897.44922243722}

func TestPropagate(t *testing.T) {
	r0, v0 := Vec3{7e6, 0, 0}, Vec3{0, 7800, 1000}
	el := FromState(MU, r0, v0)
	r, v := Propagate(MU, r0, v0, el.Period(MU))
	if r.add(r0.scale(-1)).len() > 1e-3 || v.add(v0.scale(-1)).len() > 1e-6 {
		t.Fatalf("one period: %v %v", r, v)
	}
	r, v = Propagate(MU, r0, v0, 1234)
	t.Logf("r(1234) = %.17g", r)
	if math.Abs(FromState(MU, r, v).Energy-el.Energy) > 1e-6 {
		t.Fatal("energy drift")
	}
	// Independent check: RK4 integration.
	x, u := r0, v0
	acc := func(p Vec3) Vec3 { return p.scale(-MU / math.Pow(p.len(), 3)) }
	h := 0.01
	for i := 0; i < 123400; i++ {
		k1v, k1x := acc(x), u
		k2v, k2x := acc(x.add(k1x.scale(h/2))), u.add(k1v.scale(h/2))
		k3v, k3x := acc(x.add(k2x.scale(h/2))), u.add(k2v.scale(h/2))
		k4v, k4x := acc(x.add(k3x.scale(h))), u.add(k3v.scale(h))
		x = x.add(k1x.add(k2x.scale(2)).add(k3x.scale(2)).add(k4x).scale(h / 6))
		u = u.add(k1v.add(k2v.scale(2)).add(k3v.scale(2)).add(k4v).scale(h / 6))
	}
	if d := x.add(r.scale(-1)).len(); d > 1e-2 {
		t.Fatalf("rk4 mismatch %v m", d)
	}
	if r.add(propVecR.scale(-1)).len() > 1e-4 {
		t.Fatalf("shared vector: %.17g", r)
	}
}

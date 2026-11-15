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

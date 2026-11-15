// Package orbit mirrors client/src/orbit.rs. Both are checked against shared test vectors.
package orbit

import "math"

const (
	MU     = 3.986004418e14
	REarth = 6_371_000.0
)

type Vec3 [3]float64

func (a Vec3) cross(b Vec3) Vec3 {
	return Vec3{a[1]*b[2] - a[2]*b[1], a[2]*b[0] - a[0]*b[2], a[0]*b[1] - a[1]*b[0]}
}
func (a Vec3) dot(b Vec3) float64 { return a[0]*b[0] + a[1]*b[1] + a[2]*b[2] }
func (a Vec3) len() float64       { return math.Sqrt(a.dot(a)) }

type Elements struct {
	A, E, Energy, PeriapsisR, ApoapsisR float64
}

func FromState(mu float64, r, v Vec3) Elements {
	rn := r.len()
	energy := v.dot(v)/2 - mu/rn
	h := r.cross(v)
	vh := v.cross(h)
	var ev Vec3
	for i := range ev {
		ev[i] = vh[i]/mu - r[i]/rn
	}
	e := ev.len()
	if energy >= 0 {
		p := h.dot(h) / mu
		return Elements{math.Inf(1), e, energy, p / (1 + e), math.Inf(1)}
	}
	a := -mu / (2 * energy)
	return Elements{a, e, energy, a * (1 - e), a * (1 + e)}
}

func (el Elements) Period(mu float64) float64 {
	return 2 * math.Pi * math.Sqrt(math.Pow(el.A, 3)/mu)
}

// SolveKepler solves M = E - e sin E for E (elliptic).
func SolveKepler(m, e float64) float64 {
	m = math.Mod(m, 2*math.Pi)
	if m < 0 {
		m += 2 * math.Pi
	}
	ea := m
	if e >= 0.8 {
		ea = math.Pi
	}
	for i := 0; i < 50; i++ {
		d := (ea - e*math.Sin(ea) - m) / (1 - e*math.Cos(ea))
		ea -= d
		if math.Abs(d) < 1e-13 {
			break
		}
	}
	return ea
}

func CircularSpeed(mu, r float64) float64 { return math.Sqrt(mu / r) }

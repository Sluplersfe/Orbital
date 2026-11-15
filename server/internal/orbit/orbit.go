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

func (a Vec3) scale(k float64) Vec3 { return Vec3{a[0] * k, a[1] * k, a[2] * k} }
func (a Vec3) add(b Vec3) Vec3      { return Vec3{a[0] + b[0], a[1] + b[1], a[2] + b[2]} }

// Propagate advances a bound two-body state by dt seconds (f and g functions in the change of
// eccentric anomaly). This is how on-rails ships move; it mirrors orbit::propagate in Rust.
func Propagate(mu float64, r0, v0 Vec3, dt float64) (Vec3, Vec3) {
	rn := r0.len()
	a := 1 / (2/rn - v0.dot(v0)/mu)
	sqa := math.Sqrt(a)
	sig := r0.dot(v0) / math.Sqrt(mu)
	n := math.Sqrt(mu / (a * a * a))
	m := math.Mod(n*dt, 2*math.Pi)
	de := m
	for i := 0; i < 60; i++ {
		s, c := math.Sincos(de)
		f := de + sig/sqa*(1-c) - (1-rn/a)*s - m
		d := f / (1 + sig/sqa*s - (1-rn/a)*c)
		de -= d
		if math.Abs(d) < 1e-14 {
			break
		}
	}
	s, c := math.Sincos(de)
	r := a + (rn-a)*c + sig*sqa*s
	f := 1 - a/rn*(1-c)
	g := m/n - math.Sqrt(a*a*a/mu)*(de-s)
	fd := -math.Sqrt(mu*a) / (r * rn) * s
	gd := 1 - a/r*(1-c)
	return r0.scale(f).add(v0.scale(g)), r0.scale(fd).add(v0.scale(gd))
}

func (a Vec3) Len() float64 { return a.len() }

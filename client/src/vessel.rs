//! Vessel design: stages of parts, with per-stage mass, TWR and delta-v.
use crate::parts::{self, Kind};
use crate::{G0, MU, R_EARTH};

/// Stage 0 fires first (bottom of the rocket). Each stage is dropped when staged.
#[derive(Debug, Clone, Default)]
pub struct Vessel {
    pub stages: Vec<Vec<&'static str>>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StageStats {
    pub dry: f64,
    pub prop: f64,
    pub thrust_kn: f64,
    pub isp_sl: f64,
    pub isp_vac: f64,
    /// Mass of this stage plus everything above, full / burned out (kg).
    pub m0: f64,
    pub mf: f64,
    pub dv_vac: f64,
    pub twr_sl: f64,
    pub twr_vac: f64,
    pub burn_s: f64,
}

impl Vessel {
    pub fn parts(&self, stage: usize) -> impl Iterator<Item = &'static parts::Part> + '_ {
        self.stages[stage].iter().filter_map(|id| parts::get(id))
    }

    pub fn stage_dry(&self, s: usize) -> f64 {
        self.parts(s).map(|p| p.mass).sum()
    }

    pub fn stage_prop(&self, s: usize) -> f64 {
        self.parts(s).map(|p| if let Kind::Tank { prop_kg } = p.kind { prop_kg } else { 0.0 }).sum()
    }

    /// (thrust kN, thrust-weighted Isp sl, Isp vac) of the engines in stage s.
    pub fn stage_engines(&self, s: usize) -> (f64, f64, f64) {
        let (mut t, mut dsl, mut dvac) = (0.0, 0.0, 0.0);
        for p in self.parts(s) {
            if let Kind::Engine { thrust_kn, isp_sl, isp_vac } = p.kind {
                t += thrust_kn;
                dsl += thrust_kn / isp_sl;
                dvac += thrust_kn / isp_vac;
            }
        }
        if t == 0.0 { (0.0, 0.0, 0.0) } else { (t, t / dsl, t / dvac) }
    }

    pub fn total_mass(&self) -> f64 {
        (0..self.stages.len()).map(|s| self.stage_dry(s) + self.stage_prop(s)).sum()
    }

    pub fn stats(&self) -> Vec<StageStats> {
        let n = self.stages.len();
        (0..n)
            .map(|s| {
                let above: f64 = (s + 1..n).map(|i| self.stage_dry(i) + self.stage_prop(i)).sum();
                let (thrust_kn, isp_sl, isp_vac) = self.stage_engines(s);
                let (dry, prop) = (self.stage_dry(s), self.stage_prop(s));
                let (m0, mf) = (above + dry + prop, above + dry);
                let dv_vac = if thrust_kn > 0.0 && prop > 0.0 { isp_vac * G0 * (m0 / mf).ln() } else { 0.0 };
                let mdot = if isp_vac > 0.0 { thrust_kn * 1000.0 / (isp_vac * G0) } else { 0.0 };
                StageStats {
                    dry, prop, thrust_kn, isp_sl, isp_vac, m0, mf, dv_vac,
                    twr_sl: thrust_kn * 1000.0 / (m0 * G0),
                    twr_vac: thrust_kn * 1000.0 / (m0 * G0),
                    burn_s: if mdot > 0.0 { prop / mdot } else { 0.0 },
                }
            })
            .collect()
    }

    pub fn total_dv(&self) -> f64 {
        self.stats().iter().map(|s| s.dv_vac).sum()
    }

    /// Weakest part (id, name, max-q Pa) over the whole vessel.
    pub fn weakest(&self) -> Option<(&'static str, f64)> {
        (0..self.stages.len())
            .flat_map(|s| self.parts(s))
            .min_by(|a, b| a.strength_pa.total_cmp(&b.strength_pa))
            .map(|p| (p.name, p.strength_pa))
    }

    /// Seats on board: pods and habitat modules.
    pub fn crew_capacity(&self) -> u32 {
        (0..self.stages.len()).flat_map(|s| self.parts(s)).map(|p| match p.kind {
            Kind::Pod { crew } | Kind::Habitat { crew, .. } => crew as u32,
            _ => 0,
        }).sum()
    }

    /// Food and water carried (kg).
    pub fn supplies(&self) -> (f64, f64) {
        (0..self.stages.len()).flat_map(|s| self.parts(s)).fold((0.0, 0.0), |(f, w), p| match p.kind {
            Kind::Supply { food_kg, water_kg } => (f + food_kg, w + water_kg),
            _ => (f, w),
        })
    }

    pub fn has(&self, f: impl Fn(&Kind) -> bool) -> bool {
        (0..self.stages.len()).any(|s| self.parts(s).any(|p| f(&p.kind)))
    }

    /// Drag coefficient: a nose cone streamlines the stack.
    pub fn cd(&self) -> f64 {
        if self.has(|k| matches!(k, Kind::NoseCone)) { 0.35 } else { 0.7 }
    }

    /// Problems that stop a launch.
    pub fn validate(&self) -> Vec<String> {
        let mut v = vec![];
        if self.stages.is_empty() || self.stages.iter().all(|s| s.is_empty()) {
            v.push("Vessel is empty.".into());
            return v;
        }
        if !self.has(|k| matches!(k, Kind::Pod { .. })) {
            v.push("No pod: nothing to fly or recover.".into());
        }
        if !self.has(|k| matches!(k, Kind::Engine { .. })) {
            v.push("No engine.".into());
        }
        if let Some(st) = self.stats().first() {
            if st.thrust_kn == 0.0 {
                v.push("Stage 1 has no engine, so nothing fires at liftoff.".into());
            }
        }
        v
    }
}

/// Rough delta-v to reach a 200 km orbit from the pad incl. gravity/drag losses (m/s).
pub fn orbit_dv_budget() -> f64 {
    let r = R_EARTH + 200_000.0;
    (MU / r).sqrt() + 1_500.0 - 400.0
}

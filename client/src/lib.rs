//! Orbital client core: pure, headless game logic (64-bit physics), no rendering.
pub mod autopilot;
pub mod net;
pub mod orbit;
pub mod parts;
pub mod progress;
pub mod report;
pub mod sim;
pub mod vessel;

pub const G0: f64 = 9.80665;
/// Earth: gravitational parameter (m^3/s^2), radius (m).
pub const MU: f64 = 3.986004418e14;
pub const R_EARTH: f64 = 6_371_000.0;
/// Atmosphere: sea-level density (kg/m^3), scale height (m), top (m).
pub const RHO0: f64 = 1.225;
pub const SCALE_H: f64 = 7_000.0;
pub const ATMO_TOP: f64 = 70_000.0;
/// Altitude counted as "space" (m).
pub const SPACE_ALT: f64 = 100_000.0;

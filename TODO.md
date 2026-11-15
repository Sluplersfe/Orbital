# Orbital: TODO

Tracks the work for the current phase. See [PHASES.md](PHASES.md) for the phase list and gates.

## Phase 2: Single-player slice

Completed:

- [x] Rust orbit math (`orbit.rs`): elements from state, Kepler solver, with a shared test vector for the Go module
- [x] 64-bit vessel sim (`sim.rs`): thrust, gravity, atmosphere, drag, staging, max-Q, parachute, landing
- [x] Part catalog and vessel stats (`parts.rs`, `vessel.rs`): per-stage mass, TWR, delta-v
- [x] Builder with live delta-v readout, stage editing and research-gated parts
- [x] Basic cockpit: gauges for altitude, speed, throttle, propellant, apoapsis and periapsis (after 20 km)
- [x] Eras 1-2: suborbital hop unlocks era 2 parts; orbit milestone
- [x] Flight reports that explain failures (`report.rs`) and research points (`progress.rs`)
- [x] Integration tests: era 1 hop, scripted orbit, failure explanations

Open:

- [ ] Decide the playtesting approach and record it in PHASES.md (needed before phase 3)
- [ ] Save and load progress to disk (not required by the gate)
- [ ] Write the Go orbit module against the shared test vectors (phase 3 server work)

## Phase 1: Bootstrapping

Completed:

- [x] Create the `client/` Rust crate `orbital-client` with a minimal Bevy 0.20 app that opens an "Orbital" window
- [x] Create the `server/` Go module `orbital/server` with a stub that binds TCP 4004 and UDP 4005
- [x] Add the root `README.md` with the layout and run commands
- [x] Add a root `.gitignore` for build output, server binaries and local saves
- [x] Add `CLAUDE.md` and `AGENTS.md` with the git workflow and check commands
- [x] Commit the bootstrap and push it to the `bootstrap` branch
- [x] Add `PHASES.md` and `TODO.md`, and point the agent instructions at them
- [x] Verify the client opens the window and the server binds 4004 and 4005
- [x] Align the PLAN.md phase 1 gate with PHASES.md (playtesting optional)

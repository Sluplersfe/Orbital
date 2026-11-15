# Orbital: TODO

Tracks the work for the current phase. See [PHASES.md](PHASES.md) for the phase list and gates.

## Phase 3: Co-op foundation

Completed:

- [x] Go orbit module (`server/internal/orbit`) against the shared test vectors
- [x] Join tokens, player roster, host election (earliest-joined healthy) and world clock that freezes when empty (`server/internal/session`)
- [x] Shared protobuf schema draft (`proto/orbital.proto`)

Open:

- [ ] Wire sessions into the TCP lobby and UDP state relay in `orbital-server`
- [ ] Generate Rust and Go types from the schema in CI
- [ ] Pilot-owned ships, seat claims and crew roles
- [ ] Host handoff snapshot flow and client networking
- [ ] Dockerfile and Compose for `hermes`, port forwarding
- [ ] Decide the playtesting approach and record it in PHASES.md
- [ ] Gate: 4+ players fly 30 minutes on `hermes`, forced handoff causes no ship jump, empty server freezes the clock

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

- [ ] Save and load progress to disk (not required by the gate)

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

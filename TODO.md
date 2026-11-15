# Orbital: TODO

Tracks the work for the current phase. See [PHASES.md](PHASES.md) for the phase list and gates.

## Phase 3: Co-op foundation

Completed:

- [x] Go orbit module (`server/internal/orbit`) against the shared test vectors
- [x] Join tokens, player roster, host election (earliest-joined healthy) and world clock that freezes when empty (`server/internal/session`)
- [x] Shared protobuf schema draft (`proto/orbital.proto`)

- [x] Rails propagator (`orbit::propagate` / `orbit.Propagate`) with a shared test vector, checked against RK4
- [x] Server relay (`server/internal/server`): TCP lobby with length-prefixed envelopes, UDP pilot-state relay with per-session keys and sanity checks
- [x] Pilot-owned ships, seat claims and crew roles (one player per role per ship)
- [x] Host election from measured ping RTT and loss, HostChange broadcast with a server-cached snapshot (survives abrupt host death)
- [x] Unpiloted ships go on rails when in stable orbit, otherwise freeze
- [x] Tokens file plus `orbital-server token NAME`
- [x] Rust client networking (`client/src/net.rs`) and opt-in co-op in the game (`ORBITAL_SERVER`, `ORBITAL_TOKEN`); time warp disabled online
- [x] Codegen: prost via `build.rs`, Go via `make proto`; CI workflow checks generated code, runs tests
- [x] Dockerfile and `deploy/compose.yaml`, image builds locally
- [x] Automated gate checks: Go 4-client test (forced handoff, no rails jump, empty clock freeze) and a Rust-to-Go test

Open:

- [ ] Deploy to `hermes`, forward 4004/tcp and 4005/udp, open the firewall (needs your go-ahead)
- [ ] Run the real gate: 4+ players fly 30 minutes on `hermes`
- [ ] Crew-role consoles in the client (navigator, flight engineer, mission specialist views); roles exist on the wire only
- [ ] Rate-limit client messages; tune host health thresholds (150 ms / 5% are guesses)
- [ ] Persist the world clock and rails with the world save
- [ ] Decide the playtesting approach and record it in PHASES.md

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

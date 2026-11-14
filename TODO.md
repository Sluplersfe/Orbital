# Orbital: TODO

Tracks the work for the current phase. See [PHASES.md](PHASES.md) for the phase list and gates.

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

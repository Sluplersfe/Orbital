# Orbital: phases

Four phases, built in order. Each phase ends in a gate: do not start the next phase until the gate passes. The scope of each phase comes from [PLAN.md](PLAN.md). Phases 3 and 4 of the plan are merged here into phase 4.

| # | Phase | Scope | Gate to leave the phase |
| --- | --- | --- | --- |
| 1 | Bootstrapping | Repo layout, Bevy client and Go server skeletons, build commands, git workflow and agent instructions | Client and server both build, the server binds TCP 4004 and UDP 4005, and the bootstrap branch is pushed |
| 2 | Single-player slice | Rust orbit math and 64-bit vessel sim, builder with delta-v readout, basic cockpit, Eras 1-2, flight reports, research points | Playtesters who have never seen the game reach orbit within an hour and can say why failures happened |
| 3 | Co-op foundation | Go server, shared protobuf schema, join tokens, pilot-owned ships, host and handoff, crew roles, `hermes` deployment | 4+ players fly for 30 minutes on `hermes`, a forced host handoff causes no ship jump, and an empty server freezes the clock |
| 4 | Stations, Moon and planets | Docking, station modules, refueling, surveying, resource claims, employees, life support, Eras 3-5, invented planets, tech tree balance | A group builds a station, refuels a ship there, lands on the Moon, claims a resource, and a full playthrough from Era 1 to a planet claim has no dead ends |

## Status

- Phase 1 (Bootstrapping): in progress. Tracked in [TODO.md](TODO.md).
- Phases 2-4: not started.

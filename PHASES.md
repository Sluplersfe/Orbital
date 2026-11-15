# Orbital: phases

Four phases, built in order. Each phase ends in a gate: do not start the next phase until the gate passes. The scope of each phase comes from [PLAN.md](PLAN.md). Phases 3 and 4 of the plan are merged here into phase 4.

| # | Phase | Scope | Gate to leave the phase |
| --- | --- | --- | --- |
| 1 | Bootstrapping | Repo layout, Bevy client and Go server skeletons, build commands, git workflow and agent instructions | Client and server both build, the server binds TCP 4004 and UDP 4005, and the bootstrap branch is pushed |
| 2 | Single-player slice | Rust orbit math and 64-bit vessel sim, builder with delta-v readout, basic cockpit, Eras 1-2, flight reports, research points | Reaching orbit is possible within an hour and each failure produces a flight report that explains it. Playtesting is not required here (see below) |
| 3 | Co-op foundation | Go server, shared protobuf schema, join tokens, pilot-owned ships, host and handoff, crew roles, `hermes` deployment | 4+ players fly for 30 minutes on `hermes`, a forced host handoff causes no ship jump, and an empty server freezes the clock |
| 4 | Stations, Moon and planets | Docking, station modules, refueling, surveying, resource claims, employees, life support, Eras 3-5, invented planets, tech tree balance | A group builds a station, refuels a ship there, lands on the Moon, claims a resource, and a full playthrough from Era 1 to a planet claim has no dead ends |

## Playtesting

Playtesting is not strictly necessary during the phases. Gates are checked by us, against the criteria above.

The other option is to hold all playtesting after phase 4 is complete: playtesters play the full game, and their feedback is applied in a final pass. Choose one approach before phase 2 starts, and record the choice here.

## Status

- Phase 1 (Bootstrapping): complete. Gate met: client opens the window, server binds 4004 and 4005, bootstrap branch pushed. Tracked in [TODO.md](TODO.md).
- Phase 2 (Single-player slice): complete. Gate met headlessly: `cargo test` flies a scripted ascent to a stable orbit in about 10 minutes of flight time, and every failure mode tested (never lifted, max-Q, crash) produces a diagnosing flight report. The playtesting choice above is still unrecorded and needs a decision before phase 3.
- Phase 3 (Co-op foundation): in progress. Done so far: Go orbit module, join tokens, host election, clock freeze (unit tested), schema draft. Networking, crew roles and `hermes` deployment remain.
- Phase 4: not started.

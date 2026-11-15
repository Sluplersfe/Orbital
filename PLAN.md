# Orbital: Game Design Plan

Nov 14, 2026 · @M

## Vision and pillars

Orbital is a 3D co-op PC sandbox where you design real-physics rockets, fly them yourself from the pilot seat to reach orbit, then build a space program: stations that launch better rockets, which reach new planets whose resources you research and claim, which unlock better rockets.

Four pillars decide every feature call:

1. **Design is the game.** The rocket builder is the main screen. Every part has real mass, thrust and fuel, and the player's choices decide success.
2. **Real physics, taught gently.** Orbital mechanics, staging and delta-v are simulated honestly. The game explains them as the player needs them, through in-game tools such as a delta-v readout and a maneuver planner.
3. **Every launch pays forward.** Missions return research points, claimed resources or infrastructure, so no flight is wasted, even a failure that yields data.
4. **Progress loops outward.** Researching a world claims its resources, which unlock the parts needed to reach the next one.

The target player is a Kerbal Space Program fan who wants more depth in piloting and flight planning, or a newcomer who is curious about how spaceflight really works.

**No budget.** Money is unlimited, and building or launching a rocket never consumes resources. A resource is claimed by researching it, and a claimed resource unlocks parts and tech instead of being spent.

## Core gameplay loop

The minute-to-minute loop is design, launch, fly, recover. The long loop wraps it in research and expansion.

**Short loop (10-30 minutes):**

1. Pick a mission goal, such as reaching a 200 km orbit, deploying a satellite or landing a probe on the Moon.
2. Design a rocket in the builder. The live readout shows mass, thrust-to-weight ratio and delta-v per stage.
3. Test it on the pad and in a simulated flight preview.
4. Launch and fly. The player controls ascent, staging, the orbital burn and any maneuvers.
5. Collect the payoff: research points and claimed resources, plus a flight report that shows why the rocket succeeded or failed.

**Long loop (hours):**

1. Early launches earn research points that unlock bigger rockets.
2. Rockets lift modules that form a space station.
3. The station refuels ships and assembles larger vehicles in orbit, which removes the limits of a single launch.
4. Those ships travel to the Moon, asteroids and other planets and research what they hold.
5. Claiming a resource unlocks better engines, lighter structures and new part types, and the loop repeats at a larger scale.

Failure is part of the design. A crashed rocket gives a recoverable lesson (a telemetry replay and some research points) and does not wipe out progress. Money is unlimited, so failure costs only time.

## The pilot seat

The player is the pilot, not a camera over the rocket. Flying happens in a cockpit where the controls and instruments are physical objects you operate yourself.

**Controls the player operates:**

- A control stick or wheel for pitch, yaw and roll, plus a throttle lever and a staging switch with a safety cover
- Switches for RCS thrusters, landing gear, parachutes, solar panels and docking
- Mouse-clickable cockpit with keyboard, gamepad and joystick (HOTAS) support

**Instruments the player reads and sets:**

- Altimeter, speedometer, attitude indicator (navball) and fuel and power gauges
- Apoapsis and periapsis readouts, and an orbit display
- A navigation computer where the player plots trajectories by hand: dial in the prograde, radial and normal burn values, set the burn time, and watch the predicted orbit change

**Start with basic technology.** In Era 1 the cockpit has a few analog gauges and no map, so the player flies by feel and by simple numbers. Research and contracts then unlock better instruments: a flight computer with predicted orbits, a maneuver planner, transfer-window tools, and finally an optional autopilot. Each upgrade removes some manual work without removing the pilot's role.

The builder from the next section is still the design workshop. The cockpit is where those designs are tested, so a rocket that flies badly can be traced to a specific design choice.

## Co-op multiplayer

Orbital is built as a co-op game from the start. A group of players shares one persistent space program and one world on a dedicated server, with no cap on the number of players.

- **Crew roles on one ship.** One player pilots. Others take seats as navigator (plots trajectories and burns on the nav computer), flight engineer (fuel, power and thermal systems) or mission specialist (docking, deployment, surveying).
- **Many ships, one program.** Players can fly separate vessels, dock them, and hand off cargo between ships and stations.
- **Shared base-building.** Everyone works on the same stations, claimed resources and research.
- **Mission control.** A player who is not flying can run the builder, queue launches and manage the station.

The client is authoritative, and each thing has one owner. Every player sends the server what they are working on: their position, what they built and what they researched. The server relays an owner's actual state to everyone else, and the other clients predict it between updates. The Go server does only light prediction and never overrules an owner. Ships coasting in orbit are described by orbital elements, which are tiny to send and identical on every client, so only ships under thrust or in the atmosphere need frequent updates. With no player cap, the server sends each client only what is near it or relevant to it (interest management).

**Who simulates what.** A piloted ship is simulated by its pilot's client, which is authoritative for it. Everything nobody is piloting (the world clock, coasting ships, stations and their life support) is run by one host client, the earliest-joined player with a healthy connection (latency and packet-loss thresholds). Unpiloted ships are on rails, described by orbital elements, so the host's state is small. A host handoff is a snapshot of the clock and station state, and no ship jumps because no ship's position depends on the host. If a pilot disconnects, an orbiting ship goes on rails and any other ship is frozen in place until a player takes the seat.

**No time warp.** The game runs in real time. The world clock advances only while at least one player is online, and it stops when everyone is offline, so nothing happens to ships, farms or food while the server is empty.

## Rocket design and flight systems

Rockets are assembled from parts, and the same part definitions drive both the builder UI and the physics.

| Part category | Key stats | Design tension |
| --- | --- | --- |
| Engines | Thrust (kN), Isp sea level and vacuum (s), mass, gimbal range | High thrust suits liftoff, high Isp suits space |
| Fuel tanks | Capacity (kg), dry mass, propellant type | Bigger tanks add delta-v but also weight |
| Structure | Mass, strength, diameter | Strong parts survive max-Q but cost payload |
| Control | Reaction wheels, RCS, fins, guidance computers | Needed for stability and docking |
| Aerodynamics | Drag coefficient, fairings, nose cones | Shapes the ascent through the atmosphere |
| Payloads | Satellites, crew pods, station modules, landers | The reason the rocket exists |
| Utility | Docking ports, solar panels, batteries, heat shields | Enable stations and reentry |

**Core physics to simulate:**

- Thrust, gravity, atmospheric density and drag during ascent
- Staging, with mass drop and engine ignition
- Orbital motion, with patched-conic gravity (one body's gravity at a time) for stable, cheap long-range flight
- Maneuver nodes: prograde and retrograde burns, plane changes and transfers
- Reentry heating and landing

The delta-v equation is the backbone of the design UI, and the builder shows it for every stage:

```latex
\Delta v = I_{sp} \, g_0 \ln\left(\frac{m_0}{m_f}\right)
```

**Builder aids:** a delta-v and thrust-to-weight readout per stage, a center-of-mass and center-of-pressure overlay, symmetry tools, saved blueprints and a simple auto-staging helper for beginners.

## Progression: stations, planets, resources and tech

Progress comes in five eras. Each era adds a new capability and a new bottleneck that pushes the player toward the next one.

| Era | Goal | New capability | Bottleneck it creates |
| --- | --- | --- | --- |
| 1. Suborbital | Reach space and recover the rocket | Basic engines, parachutes, simple staging | Not enough delta-v for orbit |
| 2. Orbit | Reach stable low Earth orbit | Multi-stage rockets, satellites, crew capsules | Payloads are limited by one-launch mass |
| 3. Stations | Build a station in orbit | Docking, modules, solar power, orbital refueling | Bigger modules need parts the tech tree has not unlocked |
| 4. Moon and asteroids | Send crews to survey and claim resources | Landers, surveying gear, in-orbit assembly | Deep-space trips need better engines |
| 5. Planets | Reach Mars and beyond | Transfer windows, nuclear and ion engines, long-duration habitats | Distance, timing and long-duration flight |

**Resources:** resources are never spent or required. A resource is claimed by researching it, and research means being there: a player, or an employee that a player flies out, has to travel to the body and survey it. Uncrewed probes and satellites never claim anything; they earn research points only. The claim is shared by every player and unlocks parts and tech. Money is unlimited.

- **Basic:** metals, water and silicon (Earth, Moon, asteroids)
- **Fuel:** hydrogen, oxygen and methane, made from water (claiming water ice unlocks in-space refueling)
- **Rare:** helium-3, platinum-group metals and exotic alloys (asteroids and outer planets)

**Stations** are the game's bases. Modules add power, storage, assembly bays and crew housing, and a station can assemble and fuel a ship in orbit at no cost.

**Tech tree:** research points come from science missions and unlock parts in branches (propulsion, structures, life support, automation). Claiming rare resources gates the top tier of each branch, so the player has to explore outward to move up.

**The solar system:** the real one, with real masses, distances and transfer windows, plus extra outer planets so there is somewhere to go after Neptune. The added planets are invented, and the real bodies are left as they are.

**Crews and life support:** research needs a presence. A player, or an employee, has to travel to a place and survey it. Employees are recruited at no cost once the crew-training node in the life-support branch is researched. They must be flown there by a player, with one exception: simple maneuvers they can do alone are de-orbiting (the landing spot is not guaranteed), leaving the current sphere of influence, and homing in on a new one, such as a moon to a planet or a planet to a star. Players can always teleport to Earth or to another player. Ships carry food and water, which run down on long trips. Oxygen is unlimited, so crews never suffocate. Long trips and distant bases need space farms (modules that grow food and recycle water). If a farm or habitat fails, you or your employees die and are sent back to Earth.

## Technical approach

The hardest technical problem is simulating large distances and long timescales accurately. Solve that first, because it shapes everything else.

**Stack: Rust + Bevy client, Go server.** Bevy's entity-component-system model suits a part-based vessel and cockpit rendering. The Go server relays state between players, does light prediction, and handles the lobby and saves, and it is a single static binary that is easy to run in Docker. The simulation itself runs on the host player's client. The cost is that the two sides cannot share code, so anything both must compute identically (orbit math, vessel stats) is written twice and checked against shared test vectors. Candidates below need checking before you commit:

| Layer | Choice | Note |
| --- | --- | --- |
| Client | Rust + Bevy | Rendering, cockpit instruments, input, and the host's physics |
| Server | Go | Relay, light prediction, lobby, saves |
| Wire format | Protobuf with one shared schema | Generate Rust and Go types from the same file |
| Transport | TCP 4004 reliable, UDP 4005 state | Own reliability layer on UDP, or a library such as kcp-go |
| Vessel physics | Own 64-bit sim in Rust, run by the host client | A rocket is one rigid body built from parts, so skip a general physics engine |
| Orbit math | Same Keplerian module in Go and Rust | Shared test vectors keep both identical |
| Large-world precision | big\_space on the client | Floating origin for rendering |
| Cockpit instruments | Bevy render-to-texture | Gauges and screens drawn onto cockpit panels |

**Key technical decisions:**

- **Floating origin and double precision.** Single-precision floats break down at planetary distances. Keep the physics in double precision and re-center the rendered scene around the active vessel.
- **Patched conics for orbits.** Each ship is affected by one body's gravity at a time. This is cheap and stable. Full n-body simulation is too costly and unstable for the core game.
- **Two physics modes.** Use a rigid-body simulation near the ground and in the atmosphere, and "on rails" analytic orbits when the ship is coasting.
- **Part-based vessel model.** Each part is a data asset (mass, drag, stats, attachment nodes), and a rocket is a tree of parts. This keeps content easy to add.
- **Data-driven content.** Resources, contracts and the tech tree live in data files, so balancing never needs a code change.
- **Save format.** Save the vessel tree, orbital elements and station state as plain structured data, so saves stay small and versions can be migrated.

## Hosting and deployment

The Go dedicated server runs in Docker Compose on the SSH host `hermes`. TCP port 4004 carries lobby, login and reliable messages, and UDP port 4005 carries real-time game state. Players reach it at 76.179.76.99:4004 (TCP) and 76.179.76.99:4005 (UDP). This is a plan only: nothing has been deployed or checked on `hermes` yet.

```yaml
services:
  orbital-server:
    build: .
    image: orbital-server:latest
    restart: unless-stopped
    ports:
      - "4004:4004/tcp"
      - "4005:4005/udp"
    volumes:
      - ./saves:/data/saves
```

- [x] Multi-stage Dockerfile: build a static Go binary in a golang image, run it in a minimal image as a non-root user
- [x] Shared protobuf schema in the repo, with Go and Rust code generated from it in CI
- [ ] Forward 4004/tcp and 4005/udp from the router to `hermes`, and allow them in the host firewall
- [ ] Persist world saves in the mounted volume and back them up
- [ ] Save the world clock with the world, and stop it when the last player leaves
- [x] Require a join token or allowlist, since the ports are open to the internet
- [ ] Rate-limit and size-check every client message
- [ ] Log rotation and a health check so the container restarts cleanly

## Roadmap and milestones

| Phase | Scope | Gate to leave the phase |
| --- | --- | --- |
| 0. Prototype | Rust orbit math and 64-bit vessel sim, plain rendering, one ship, no UI polish | A rocket reaches orbit, and the Rust and Go orbit modules match on every shared test vector |
| 1. Single-player slice | Builder with delta-v readout, basic cockpit, Eras 1-2, flight reports, research points | Reaching orbit is possible within an hour, and each failure produces a flight report that explains it. Playtesting is optional here (see [PHASES.md](PHASES.md)) |
| 2. Co-op foundation | Go server, shared protobuf schema, join tokens, pilot-owned ships, host and handoff, crew roles, `hermes` deployment | 4+ players fly for 30 minutes on `hermes`, a forced host handoff causes no ship jump, and an empty server freezes the clock |
| 3. Stations and Moon | Docking, station modules, refueling, surveying, resource claims, employees, life support and farms, Eras 3-4 | A group builds a station, refuels a ship there, lands on the Moon and claims a resource |
| 4. Planets and content | Era 5, invented planets, nuclear and ion engines, tech tree balance, contracts, instrument upgrades | A full playthrough from Era 1 to a planet claim has no dead ends, and balance playtests pass |

Each phase ends in a gate. If a gate fails, stay in that phase and fix the weakest area before building more. Set dates only after the prototype shows how fast you can build.

## Risks and open questions

| Risk | Why it matters | Mitigation |
| --- | --- | --- |
| Learning curve is too steep | Real orbital mechanics turns many players away | Delta-v readout, maneuver planner, guided first missions |
| Scope grows too large | Builder, physics, stations and co-op networking are each a full game | Build and test one phase at a time, using the gates |
| Physics instability | Large vessels and docking can jitter or explode | Prototype physics early, keep part counts bounded |
| Research feels like a chore | Surveying every resource can become busywork | Let one survey claim a whole group of resources |
| Late-game content runs thin | Players may reach the planets quickly | Procedural contracts and a steady supply of new part types |
| Players can cheat | The server trusts what each owner sends | Join tokens and a trusted player list, with only basic sanity checks |
| Host leaves | Station life support and the world clock stall | Hand them to the next healthy player with a snapshot; piloted ships are unaffected |

**Open questions to decide next:**

- [ ] Which two to four invented planets and moons do we add, and what are their orbits?
- [ ] With no player cap, how do we limit what each client must receive?
- [ ] Are there other maneuvers an employee can fly alone, beyond de-orbiting, leaving a sphere of influence and homing in on a new one?
- [ ] How do space farms work: size, power use, yield and how much warning there is before a failure?
- [ ] What latency and packet-loss thresholds make a player ineligible to be host?

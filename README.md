# Orbital

A co-op PC space program sandbox. See [PLAN.md](PLAN.md) for the design.

## Layout

- `client/`: Rust + Bevy game client (`orbital-client`)
- `server/`: Go dedicated server (`orbital/server`), TCP 4004 and UDP 4005

## Build and run

```sh
# Client
cd client && cargo run

# Server
cd server && go run ./cmd/orbital-server
```

# Orbital: notes for AI agents

Orbital is a co-op PC space program sandbox. The design lives in [PLAN.md](PLAN.md). Layout and build commands are in [README.md](README.md).

- `client/`: Rust + Bevy game client
- `server/`: Go dedicated server

## Git workflow

- Always commit and push your work when a task is finished. Do not leave finished changes uncommitted or unpushed.
- Never commit directly to `main`. Create a branch first, then push it with `git push -u origin <branch>`.
- Commit messages: short imperative summary line, then a body if the reason is not obvious.
- Do not force-push or rewrite shared history.

## Checks before committing

- Client: `cd client && cargo check`
- Server: `cd server && go vet ./... && go build ./cmd/orbital-server`

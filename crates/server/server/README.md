# message-crate-server

HTTP API and SQLite storage for browsing imported messages. This is the server: import, export, contacts, search, auth, and attachment endpoints. It also has CLI subcommands (`serve`, `import`, `reset-demo`, and others).

The Vite SPA in `web/` is the website this server can host. Docker images wrap this crate.

## Build and test

```bash
cargo test -p message-crate-server
cargo run --release -p message-crate-server -- serve
```

Docker (release-shaped image from this checkout): `docker compose -f docker/compose.release.yml up --build`. Day-to-day from a clone: `./scripts/run-dev.sh` (see [CONTRIBUTING.md](../../../CONTRIBUTING.md)). Published image: [Try Message Crate](https://messagecrate.app/docs/user/get-started/try-message-crate/).

## Docs

- Try Message Crate: https://messagecrate.app/docs/user/get-started/try-message-crate/
- Operator Docker: https://messagecrate.app/docs/developer/docker-compose/
- Server CLI: https://messagecrate.app/docs/developer/reference/server-cli/
- API: https://messagecrate.app/docs/developer/reference/api/

## License

Fair Core License. See the repository root `LICENSE.md`.

# message-vault-server

HTTP API and SQLite storage for browsing imported messages. This is the vault: import, export, contacts, search, auth, and attachment endpoints. It also has CLI subcommands (`serve`, `import`, `reset-demo`, and others).

The Vite SPA in `web/` is the website this server can host. Docker images wrap this crate.

## Build and test

```bash
cargo test -p message-vault-server
cargo run --release -p message-vault-server -- serve
```

Docker (release-shaped image from this checkout): `docker compose -f docker/compose.release.yml up --build`. Day-to-day from a clone: `./scripts/run-vault-dev.sh` (see [CONTRIBUTING.md](../../../CONTRIBUTING.md)). Published image: [Try the vault](https://messagecrate.app/docs/user/get-started/try-the-vault/).

## Docs

- Try the vault: https://messagecrate.app/docs/user/get-started/try-the-vault/
- Operator Docker: https://messagecrate.app/docs/developer/docker-compose/
- Server CLI: https://messagecrate.app/docs/developer/reference/server-cli/
- API: https://messagecrate.app/docs/developer/reference/api/

## License

Fair Core License. See the repository root `LICENSE.md`.

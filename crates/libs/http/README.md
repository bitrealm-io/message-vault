# message-crate-http

Blocking HTTP client helpers and typed retry classification for the crates that
talk to a Message Crate server. Calls block so they can run on worker threads
without an async runtime.

One `HttpSession` carries the connection pool; `auth_check` performs the login
probe; `ok_json` reads every server answer, turning a failure into the server's
own `{error}` sentence rather than a status code; and `classify_retry` decides
which failures are worth trying again.

`message-crate-push` and `message-crate-pull` use this crate, and the desktop app reaches
`AuthError` through their re-exports.

## Build and test

```bash
cargo test -p message-crate-http
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library used by the push and pull crates. It builds no binary.

## License

Fair Core License. See the repository root `LICENSE.md`.

# vault-api-types

The response shapes the server's HTTP API sends, defined once for the server
that writes them and the client crates that read them: `Message`,
`MessageConversation`, `Participant`, `Attachment`, and `Tapback`.

Two crates sit on either side of these shapes. While each kept its own copy,
the two could disagree silently, and did — three defects shipped that way, each
a pull that failed at runtime or quietly produced worse data. One definition
makes the compiler the check instead.

`skip_serializing_if` and `default` come as a pair here and only as a pair: a
field the server may leave out is a field a reader has to do without, and a
field the server always sends stays required in the OpenAPI document rather
than turning optional in every generated client.

The `schema` feature adds `utoipa::ToSchema` so the same structs describe
themselves in the OpenAPI document. The server turns it on; client crates
leave it off and never build utoipa.

## Build and test

```bash
cargo test -p vault-api-types
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library shared by the server and `vault-pull`. It builds
no binary.

## License

Fair Core License. See the repository root `LICENSE.md`.

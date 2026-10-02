# contacts

Parses a vCard (VCF) file into cards: names, phone numbers, email and categories.

Message Crate does not read a vCard as an address book. Its address book is its own CSV, which the server writes and loads. This parser is kept for the conversion from a vCard to that CSV ([#916](https://github.com/messagecrate/message-crate/issues/916)); nothing in the workspace calls it until then.

## Build and test

```bash
cargo test -p contacts
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library. The address book: https://messagecrate.app/docs/user/features/contacts/contacts/

## License

Fair Core License. See the repository root `LICENSE.md`.

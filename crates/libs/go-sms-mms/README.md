# go-sms-mms

Decode the MMS `.pdu` files in a GO SMS Pro backup: each one is the MMS
PDU the phone's MMS stack held, byte for byte, so the crate reads them by
the WAP-209 (MMS Encapsulation) and WAP-230 (WSP) rules and nothing else.
The module documentation lists every rule with its reason: `wsp` for the
value shapes and the multipart body, `mms` for the header walk, `pdu` for
one file as one message.

`go-sms-pro-exporter` uses this crate. The `testutil` feature exposes a
builder that writes PDUs the way a phone does, for tests here and there.

## Build and test

```bash
cargo test -p go-sms-mms
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

## Docs

This crate is a library. Field mapping: https://bitrealm.io/vault/developer/formats/go-sms-pro/mapping/

## License

Fair Core License. See the repository root `LICENSE.md`.

# mms-parts

Turn the parts of an MMS into the message text and its attachments, by one
rule for every reader: SMS Backup & Restore (`sbr`), SMS Backup+
(`sms-backup-plus-exporter`) and GO SMS Pro (`go-sms-mms`). Each reader maps
its own parts into `Part` and calls `body_of`. The crate documentation lists
the rules with their reasons.

## Build and test

```bash
cargo test -p mms-parts
```

Workspace setup: [CONTRIBUTING.md](../../../CONTRIBUTING.md).

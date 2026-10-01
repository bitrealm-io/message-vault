# Message Crate demo dataset

Generated message-ir JSONL bundle for local browsing without a real phone backup.
`staging/` is written by `demo-seed` / `reset-demo` and is not stored in git.

Three staging trees simulate separate backups:

- `staging/imessage/` — Apple Messages-style export
- `staging/sms-backup-restore/` — Android SMS Backup & Restore–style export
- `staging/whatsapp/` — WhatsApp-style export for ~20% of contacts (same phone, platform `whatsapp`)

Most conversations are single-source. A small set appears in both iMessage and Android so the
Sources panel and cross-source dedupe can be exercised. WhatsApp threads share the phone number
with Text message handles so the contact drawer shows both platforms.

Regenerate + import in one step:

```bash
cargo run --release -p message-crate-server -- reset-demo
```

Or regenerate the bundle only:

```bash
cargo run -p demo-seed
```

Config knobs live in `crates/server/demo-seed/demo_seed.toml` (seed, contact count, rate/span
distributions, group membership, dual-source split, `whatsapp_contact_fraction`,
`apple_fallback_transport_fraction`). Message bodies are sampled from Pride and
Prejudice (5274 sentences) under `crates/server/demo-seed/data/corpus/`. Names come from
`crates/server/demo-seed/data/names/`.

## Contents (seed 42)

| Item | Count |
|------|------:|
| Contacts (VCF) | 200 |
| Groups | 185 |
| Conversation files | 394 |
| Messages | 612893 |
| Attachment references | 9567 |

## Exercises

- **Triple sources** — `imessage` vs `sms-backup-restore` vs `whatsapp`
- **Platform handles** — Text message + WhatsApp rows on the same contact
- **Transport mix** — SMS/RCS mixed into iMessage threads (~20% by default)
- **Contacts / groups / No Messages** — group memberships and zero-message rows
- **Unassigned** — handles with messages but no VCF row (phone + email)
- **Rate skew** — most 1:1 threads ~200–300 msgs/year (bursty days); rare whales up to ~12k/year
- **History** — typical first contact ~3–5 years ago; longest ~14 years; newest ~1 week
- **Group Chats** — membership mean ~5 groups/contact; size mean ~4; at least 10 groups with 8–20 participants; bursty days (several / none / a lot)
- **Replies, tapbacks, attachments** — including one intentionally missing file
- **orphaned.jsonl** — synthetic orphaned conversation

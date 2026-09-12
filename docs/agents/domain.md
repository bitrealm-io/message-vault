# Domain Docs

How the engineering skills should consume this repo's domain documentation when exploring the codebase.

## Before exploring, read these

- **`CONTEXT.md`** at the repo root — the glossary.
- **`docs/adr/`** — the ADRs touching the area you are about to work in.

This is a single-context repository: one `CONTEXT.md` at the root, one
`docs/adr/`.

```
/
├── CONTEXT.md
├── docs/adr/
│   ├── 0001-no-command-line-except-the-vault-server.md
│   └── 0002-one-way-to-fetch-data-in-the-web-app.md
├── crates/
├── src-tauri/
└── web/
```

The `/domain-modeling` skill adds entries lazily, when a term or a decision
actually gets resolved. Do not propose writing them up front.

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a hypothesis, a test name), use the term as defined in `CONTEXT.md`. Don't drift to synonyms the glossary explicitly avoids.

If the concept you need isn't in the glossary yet, that's a signal: either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `/domain-modeling`).

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than silently overriding:

> _Contradicts ADR-0007 (event-sourced orders), but worth reopening because…_

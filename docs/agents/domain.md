# Domain Docs

How the engineering skills should consume this repo's domain documentation when exploring the codebase. The repo has one context: one `CONTEXT.md` at the root and one `docs/adr/`.

## Before exploring, read these

- **`CONTEXT.md`** at the repo root: the glossary.
- **`docs/adr/`**: read ADRs that touch the area you're about to work in.
- **`docs/architecture/http-api.md`** when the work touches a `/v1` route. The HTTP interface has no ADRs; that one file holds every rule, its reason, and what was rejected. A rule is written there when it is decided, and a route that breaks it is a bug with an open issue (`docs/adr/0011-the-http-interface-has-one-rules-document.md`).
- **`docs/architecture/`** when the work touches the model it covers. `contacts-identities-and-messages.md` holds the relationships and rules for contacts, handles, participants, conversations and messages. A rule is written there when it is decided; where the code does not follow it yet, an open issue says so.

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a hypothesis, a test name), use the term as defined in `CONTEXT.md`. Don't drift to synonyms the glossary explicitly avoids.

If the concept you need isn't in the glossary yet, that's a signal: either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `/domain-modeling`).

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than silently overriding:

> _Contradicts ADR-0007 (CI is the only gate), but worth reopening because…_

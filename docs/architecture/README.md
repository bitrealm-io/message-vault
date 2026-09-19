# Architecture

How Message Vault is put together: the things the system holds, how they
relate, and the rules that hold between them. These documents are written for
anyone working on the product, person or AI.

Each document describes the system as it stands. A change to a rule lands here
in the same pull request as the code, so a document that disagrees with the
code is a bug in one of them.

| Document | Covers |
|---|---|
| [Contacts, identities and messages](contacts-identities-and-messages.md) | The people model: what a contact and an identity are, how conversations and messages attach to them, and what an import creates |

## What goes where

- **`CONTEXT.md`** defines the words. An architecture document uses them and
  does not redefine them.
- **`docs/adr/`** records why a decision was made and what was turned down. An
  architecture document states the result and links the ADR.
- **`docs/agents/http-api-rules.md`** holds every rule for `/v1` routes.
- **`docs/architecture/`** holds the model of the system itself.

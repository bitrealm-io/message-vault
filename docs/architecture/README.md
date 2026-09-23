# Architecture

How Message Vault is put together: the things the system holds, how they
relate, and the rules that hold between them. These documents are written for
anyone working on the product, person or AI.

Each document states the rules the system follows. A rule is written here when
it is decided, not when the code catches up, so the rules are explicit for
design as well as for review. Where the code does not follow a rule yet, an
open issue names what is still to change; any other disagreement between a
document and the code is a bug in one of them.

| Document | Covers |
|---|---|
| [Contacts, identities and messages](contacts-identities-and-messages.md) | The people model: what a contact and an identity are, how conversations and messages attach to them, and what an import creates |
| [The HTTP interface](http-api.md) | Every rule the `/v1` routes follow: identifiers, naming, methods, status codes, lists, failures, credentials, runs, the generated reference, and how the server code behind a route is named and layered |

## What goes where

- **`CONTEXT.md`** defines the words. An architecture document uses them and
  does not redefine them.
- **`docs/adr/`** records why a decision was made and what was turned down. An
  architecture document states the result and links the ADR.
- **`docs/architecture/`** holds the model of the system itself.

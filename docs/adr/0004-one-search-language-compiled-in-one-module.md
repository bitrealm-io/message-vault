# One search language, compiled in one module

The search language a person types on the Contacts, Conversations, and
Messages lists is one language, owned by one module in the vault server
(`crates/vault/server/src/search/`). That module parses every word and
compiles it to SQL for whichever list asked. The three list routes call it;
none of them parses a query string or builds a filter of its own.

The rules of the language:

- **One word per concept.** A concept has exactly one spelling, with no
  aliases. Precision lives in the value using conventional notation:
  `date:>2019`, `date:2019..2021`, `size:<500k`, `messages:>100`. There are
  no paired words like `before:`/`after:` or `larger:`/`smaller:`.
- **One meaning per word, on every list.** `group:Family` means "in this
  Contact Group" wherever it is accepted. Each list says which words it
  accepts.
- **A word that does not belong to the list is refused.** The answer is a
  400 that names the word and the list, with a "did you mean" only when the
  current word list has something close. Nothing is silently searched as
  text or dropped, and the module knows nothing about spellings that came
  before it.
- **`q` and `-q` split the list.** `-q` is every row of the list that `q`
  does not match, so a row with no value for the word, such as a contact
  with no messages under `last-message:`, is in `-q`. The only exception is
  a word that lifts one of the list's defaults (`trashed:`, `source:`,
  `import:`). A row missing from both answers is invisible to every search
  and cannot be explained to the person looking for it.
- **A query only narrows.** Sort order, context lines, one row per message
  versus per conversation, and the Contacts mode switch are request
  parameters, not words in the string.
- **Compile is pure.** The module turns a string into SQL with no database
  connection and no clock. Anything that once needed a lookup first is a
  subquery. Its tests run query strings against a seeded test vault and
  assert which rows come back.

The word table is `crates/vault/server/src/search/fields.rs`, the registry
every list compiles against and the source the API reference is generated
from. The grammar a person reads is
`docs/src/content/docs/vault/user/how-to/search.mdx`; the words each list
accepts are in the [HTTP API reference](/vault/developer/reference/api/).

## Why

The words grew by accretion in the first week of August 2026, when the goal
was to get search working at all, and were never revisited. By September the
server had three parsers for one language, one in each route file, each with
its own tokenizer, quoting, and date handling. The same concept had different
spellings on different lists (`group:` on Contacts, `people:`, `within:`, or
`label:` elsewhere), the same spelling had different meanings (`group:none`
was "in no Contact Group" on one list and "one row per message" on another),
and a word the list did not know was searched as text on one list and refused
on another. Six documented operators were refused everywhere as "not
implemented in SQL yet".

Deleting the three parsers makes the same tokenizing, quoting, and date logic
reappear three times, which is the mark of a pass-through. Putting the
language behind one interface concentrates every future change, every bug,
and every test in one place, and makes the docs page something a test can
check.

## Considered and rejected

**Keep today's words and only unify the collisions.** Faster, but it freezes
a set of accidental spellings into a module that is much harder to change
than three loose parsers were. The interface of a deep module is the
expensive part to change later, so the words were designed before it was
built.

**Email-client spellings** (`before:`, `after:`, `larger:`, `smaller:`,
`has:attachment`, an `is:` family) for familiarity with Gmail and Fastmail.
Rejected because each is two or three words for one concept, and `has:` and
`is:` were exactly the two words whose values differed on every list. A
language with one rule is easier to keep true across three lists than one
with borrowed pairs, and ranges in one token are something email search
cannot express. A Gmail habit like `before:2020` is an unknown word, the
same as a typo.

**Bare keywords and a catch-all** (`photos`, `2019`, `sent`, `direct` with
no colon; a bare word matching text, names, handles, titles, group names,
and tag names at once). Rejected because it turns ordinary words into
keywords that then need quoting, and because a bare word meaning "anything
anywhere" is the fuzziness this decision exists to remove.

**A one-time rewriter for stored Saved Search text, or any table of old
spellings for error messages.** Rejected. Either keeps the old language alive
inside the codebase, and the old language was a first pass that was always
expected to change. Stored text that no longer parses fails as an unknown
word, the same as typed text, and the person edits it once.

## Consequences

- Adding a filter is one entry in the module's field registry plus one SQL
  emitter. The web's suggestions, the docs page, and the refusal messages
  read that registry, so they cannot drift from what compiles.
- The web sends `sort`, `order`, `rows`, and `context` as request parameters
  and no longer embeds them in the query string. Its operator-sniffing
  regexes are deleted in favour of `GET /v1/search/fields`.
- Saved Searches written in spellings the language does not have fail as
  unknown words.
- The three route files shrink to their handlers and their base queries. A
  later, mechanical change can move what remains out of `*_api.rs`.

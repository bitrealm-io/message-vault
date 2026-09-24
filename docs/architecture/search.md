# The search language

The one language a person types on the Contacts, Conversations, and Messages
lists: its grammar, what every value and every word means on every list, and
the rules between them. It is written for anyone adding, changing, or
reviewing a search word, precisely enough to write a word's SQL from.

Someone who only wants to search should read the user guide,
[Search](../src/content/docs/vault/user/how-to/search.mdx), instead. Why the
language is one module with one registry is
[ADR 0004](../adr/0004-one-search-language-compiled-in-one-module.md).

The code is `crates/vault/server/src/search/`: `lex.rs` turns the string into
tokens, `parse.rs` turns tokens into a tree and resolves every word against
the registry in `fields.rs`, and `emit.rs` with `bridge.rs` writes the SQL.

## Rules

Each rule holds on every list, and each has its reason.

- **One word per concept.** A concept has exactly one spelling and no
  aliases. Precision lives in the value (`date:>2019`, `size:<500k`), never in
  paired words like `before:` and `after:`. Why: two spellings for one idea
  are two things to document, suggest, and keep in step, and a person cannot
  tell whether they mean the same thing.
- **A word is shared between lists only when it answers the same question on
  each.** `group:Family` asks "is this in the Contact Group" on every list it
  is on, reaching the contact from a conversation or a message. A word whose
  question differs on one list gets its own word there: on Contacts the
  question is when the contact wrote, so it is `last-heard:`, while
  Conversations and Messages ask when the conversation's last message was,
  which is `last-message:` (#718). Why: a shared word that answers different
  questions is read by its most natural meaning, and on the other list that
  reading is wrong.
- **A word the list does not have is refused.** The query is refused whole,
  with a problem naming the word and the list, and a "did you mean" only when
  a word on that list is within two edits. Nothing is searched as text or
  dropped. The language keeps no table of spellings it used to have. Why: a
  silently ignored word returns rows the person did not ask for, and they
  cannot see why.
- **A query only narrows.** Sort order, grouping by conversation, context
  lines, and the Contacts mode switch are request parameters, never words.
  Why: a Saved Search then holds only what to find, so it means the same thing
  on every screen that runs it.
- **Compile is pure.** Compiling reads no database and no clock. Anything that
  needs a lookup (the last Import Run, a Contact Group by name) is a subquery,
  and today's date and the account's time zone are inputs. Why: the same
  string compiles to the same SQL, so tests can pin it, and a query never
  depends on the moment it was parsed.
- **`q` and `-q` split the list.** `-q` is every row of the list that `q` does
  not match. A row with no value for the word does not match `q`, so it
  matches `-q`: `-last-heard:>=2022` includes the contacts who never sent a
  message. The one exception is a word that lifts a default (below), where
  `q` and `-q` together cover the lifted list. Positive words stay strict: a
  date word matches only rows that have that date. Why: a row missing from
  both answers is invisible to every search and cannot be explained (#698).
- **An id that names nothing matches nothing, and is not refused.** `group:#99`
  for a Contact Group that does not exist, or that belongs to another account,
  matches no row, and `-group:#99` matches every row. Why: a Saved Search that
  names a Contact Group later deleted must keep running (#698).
- **Every query is scoped to the logged-in account.** The compiled filter is
  `AND`-ed with the base row's account, so no row of another account can
  match. Why: an account's data is isolated from every other account in the
  vault.

## Grammar

A query is at most 2,048 bytes. A NUL byte reads as a space.

```ebnf
query    = [ or ] ;
or       = and , { "or" , and } ;                 (* loosest *)
and      = unary , { [ "and" ] , unary } ;        (* a space is an and *)
unary    = "not" , unary
         | [ "-" ] , primary ;                    (* tightest *)
primary  = "(" , or , ")"
         | field
         | phrase
         | term ;
field    = word , ":" , ( quoted | bare ) ;
word     = letter , { letter | "-" } ;            (* ASCII, any case *)
phrase   = quoted ;
term     = bare ;                                 (* a trailing * makes a prefix *)
quoted   = '"' , { any - '"' | '""' } , '"' ;     (* "" is one quote *)
bare     = { any - space - "(" - ")" } ;
```

In practical terms:

- `or` binds looser than `and`, and a space between two terms is an `and`, so
  `a b or c` is `(a b) or c`. Both are left-associative and flatten:
  `a or b or c` is one three-way `or`.
- `not` and `-` bind tightest. `-` negates the one token it is attached to,
  with no space between, a parenthesised group included: `-(a or b)`. `not`
  is a word of its own and negates the operand after it: `not (a or b)`.
- `or`, `and`, and `not` are operators in any case. With a `-` attached they
  are text: `-or` excludes the word "or".
- A `word:` is a field only when the word is letters and hyphens, starting
  with a letter, and the value does not start with `/`, so a pasted `http://…`
  stays a term. The word is read in any case. A field whose word the list does
  not have is refused, never searched as text: `note:x` is an unknown word.
- An unquoted value runs to the next space or parenthesis. A quoted value may
  hold anything, and `""` inside it is one quote.
- In an unquoted value a comma separates values that mean either:
  `service:imessage,sms`. In a quoted value a comma is text.
- A term ending in `*` is a prefix (`avoc*`). A quoted phrase has no prefix.

The parser refuses a query with more than 32 free-text terms, more than 64
nodes in its tree, or parentheses and `not` nested deeper than 32. An
unclosed quote or parenthesis, an `or` or `and` with nothing on one side, and
a `word:` with no value are refused with the span of the text at fault.

## Values

Every word takes one shape of value, named by its `ValueType` in `fields.rs`,
plus the keywords its registry entry lists.

| Shape | Accepts | Matches |
|---|---|---|
| Text | text, `pre*`, and `none`/`any` where listed | The column contains the text, case-insensitively. `pre*` matches the start of the column or of any word in it (after a space). `none` is empty or only spaces; `any` is its complement. |
| Name | a name, `pre*`, `#id`, and the word's keywords | `#id` is that row by id, unquoted. `group:` and `tag:` match a name equal to the text, case-insensitively, or starting with the prefix; `in:` matches a title or identity that contains the text or starts with the prefix. `import:` takes only `#id` and `last`. |
| Person | a name, a handle, `pre*`, `#id`, `me` where listed | `#id` is a contact: one of their identities, or a participant linked to them. Text is contained in an identity's raw or normalized form, in the name of the contact linked to it, or in a participant's name; `pre*` matches the start of any of those instead. |
| Choice | one of the word's fixed values | That value, compared case-insensitively. |
| Flag | `yes`, `no`, `any` | `trashed:` only. |
| Date | a span, with `>`, `>=`, `<`, `<=`, or `a..b` | See below. |
| Count | a whole number, with `=`, `>`, `>=`, `<`, `<=`, or `a..b` | A bare number is equality. A range is inclusive at both ends. |
| Size | `500k`, `1M`, `2G`, or bytes, with the Count comparisons | 1024-based units, a trailing `b` allowed, decimals rounded to whole bytes. |

A date names a span of days in the account's time zone:

- `2024` is the year, `2024-05` the month, `2024-05-01` the day, `today` and
  `yesterday` those days.
- `7d`, `2w`, `3m`, `1y` run from that long before today through the end of
  today; a month back from the 31st lands on the last day of the shorter
  month. A span reaching back further than 3,650 days, counting a month as 31
  days and a year as 365, is refused.
- A bare span matches inside it. `>=` is from its start, `<` is before its
  start, `>` is after its end, `<=` is up to its end, and `a..b` runs from the
  start of `a` through the end of `b`, which must not end before `a` begins.

A message stores the instant it was sent, in UTC. Each day's edge becomes the
instant midnight falls in the account's time zone, so the same comparison
serves SQLite and Postgres and a message sent at 11:59 pm on New Year's Eve
belongs to the old year wherever the vault runs. A midnight that falls in a
daylight-saving gap starts the day at the first instant after the gap.

Case and accents:

- Case never matters. The text, name and person words compare
  `lower(column)` with `lower(text)` on both engines, so `name:élodie` finds
  "Élodie" as `name:jane` finds "Jane". Postgres's `lower()` folds every
  letter. SQLite's folds only ASCII, and its `LIKE` and `NOCASE` collation
  fold no more, so the vault replaces `lower()` on every SQLite connection
  with one that folds Unicode (`db/sqlite_functions.rs`, registered through
  `sqlite3_auto_extension`). Message text, which goes through the full-text
  index, folds case on both engines.
- Accents matter, except in message text on SQLite, whose full-text index
  folds them: `cafe` finds "café" there and not on Postgres.
- `%`, `_`, and `\` are ordinary characters. The only wildcard is a trailing
  `*`, and only in free text and on the Text words; on a Name or Person word
  it is part of the text.
- A keyword is read as a keyword quoted or not: `group:"none"` is `group:none`.
  A Contact Group named "none" is reached by its `#id`.

Free text on Messages goes to the full-text index (SQLite's FTS5 table,
Postgres's `search_tsv`), which indexes the body, the subject, attachment file
names, and transcriptions, together with a contains match on attachment file
names. Punctuation inside a term splits it into words that must appear next
to each other in that order, and a term that is only punctuation or emoji
finds no message text.

## Lists

Each list has one base row, and every word is written as a question about it.
The three lists reach one another in the same few ways, and the word entries
below use these phrases for them:

- **The contact's conversations** are the conversations one of the contact's
  identities is in, as the conversation's own identity or a participant's, and
  the conversations with a participant the import linked to the contact
  (`participants.contact_id`), which is how a participant with no identity is
  reached.
- **The conversation's contacts** are the contacts of the conversation's
  participants, found the same way.
- **The conversation's messages** are its messages that are not duplicates.

| List | Base row | Plain text searches | Defaults | Lifted by |
|---|---|---|---|---|
| Contacts | one contact | the contact's name, and the raw and normalized form of each of its identities | a contact in the trash is left out | `trashed:` |
| Conversations | one conversation | the title, the raw form of the conversation's own identity and of each participant's identity, and each participant's name | a conversation in the trash is left out; a conversation whose every message is a duplicate is left out | `trashed:` lifts the first; `import:` lifts the second |
| Messages | one message | the full-text index (above) and attachment file names | a message whose conversation is in the trash is left out; a duplicate message is left out | `trashed:` lifts the first; `source:` and `import:` lift the second |

A word lifts its default wherever it appears in the query, negated or inside
an `or` included. Why these words: `trashed:` is the question of the trash
itself, and an Import Run or a backup source is often nothing but duplicates
of messages a later import kept, so a search about one must see them.

A participant's name is the name of the contact linked to their identity, or
else the name the source gave them in that conversation. The link through the
identity is read at search time, so a name is found the moment an identity is
linked to a contact.

What the trash on the far side of a bridge does is not decided yet. A
Contacts word that reaches the contact's conversations (`date:`, `kind:`,
`tag:`, `service:`, `conversations:`) counts a conversation in the trash, while
`messages:`, `first-heard:`, and `last-heard:` leave it out. A Conversations or
Messages word that reaches the conversation's contacts counts a contact in the
trash (#724).

## Words

One entry per word in the registry, in its order, with one line per list it
accepts. `none` and `any` are written only where the word lists them. A test
(`docs` in `search/tests.rs`) checks that this section, the user guide's word
table, and the registry name the same words on the same lists.

### `body:`

Text, `none`, `any`.

- **Conversations**: one of the conversation's messages has this body. `none` is one whose body is empty.
- **Messages**: the message's body.

### `subject:`

Text, `none`, `any`.

- **Conversations**: one of the conversation's messages has this subject line.
- **Messages**: the message's subject line.

### `name:`

Text, `none`, `any`.

- **Contacts**: the contact's own name. `none` is a contact with no name.
- **Conversations**: a participant's name. `none` is a participant with no name.
- **Messages**: a participant's name in the message's conversation.

### `title:`

Text, `none`, `any`.

- **Conversations**: the conversation's title. `none` is a conversation with no title.
- **Messages**: the title of the message's conversation.

### `handle:`

Text, `none`, `any`. The raw or the normalized form of an identity.

- **Contacts**: one of the contact's identities. `none` is a contact with no identity.
- **Conversations**: the conversation's own identity or a participant's. `none` is a conversation where no participant has an identity; `any` is one where some participant does.
- **Messages**: the same, for the message's conversation.

### `with:`

Person.

- **Conversations**: this person is in the conversation: the conversation's own identity or a participant's identity is theirs, or a participant's name contains the text, or a participant is linked to the contact `#id`. The last two reach a participant the source named and gave no identity.
- **Messages**: the same, for the message's conversation.

### `from:`

Person, `me`.

- **Messages**: `me` is a message the account holder sent. A person is a message the holder received whose sender identity matches the person, by `#id` or by text in the identity or in the name of the contact linked to it.

### `to:`

Person, `me`.

- **Messages**: `me` is any message the account holder received. A person is a message in a conversation with that person (`with:`) that the person did not send.

### `in:`

Name.

- **Messages**: `#id` is the message's conversation. Text is contained in the conversation's title or in the raw form of its own identity.

### `group:`

Name, `none`, `unknown`.

- **Contacts**: the contact is a member of this Contact Group. `none` is a contact in no Contact Group and not Unknown; `unknown` is an Unknown contact (no name, or no identity).
- **Conversations**: one of the conversation's contacts is a member. `none` is a conversation none of whose contacts is in a Contact Group or Unknown; `unknown` is one with an Unknown contact.
- **Messages**: the same, for the message's conversation.

### `tag:`

Name, `none`.

- **Contacts**: one of the contact's conversations carries this Message Tag. `none` is a contact none of whose conversations carries a tag.
- **Conversations**: the conversation carries this Message Tag. `none` is a conversation with no tag.
- **Messages**: the message's conversation carries it.

### `kind:`

Choice: `direct`, `group`.

- **Contacts**: one of the contact's conversations is of this kind.
- **Conversations**: the conversation is one-to-one (`direct`) or a group.
- **Messages**: the message's conversation is.

### `service:`

Choice: `imessage`, `sms`, `mms`, `rcs`, `whatsapp`.

- **Contacts**: one of the messages of the contact's conversations travelled this way.
- **Conversations**: one of the conversation's messages travelled this way.
- **Messages**: the message travelled this way.

### `source:`

Choice: `imessage`, `whatsapp`, `sms`. `sms` is the SMS Backup & Restore importer.

- **Conversations**: one of the conversation's messages came from this kind of backup.
- **Messages**: the message came from this kind of backup, duplicates included.

### `import:`

`#id`, `last`. `last` is the account's newest Import Run.

- **Conversations**: one of the conversation's messages, duplicates included, was brought in by this Import Run.
- **Messages**: the message was brought in by this Import Run, duplicates included.

An account with no Import Runs has no `last`, so `import:last` matches nothing
and `-import:last` matches every row.

### `date:`

Date.

- **Contacts**: one of the messages of the contact's conversations, by anyone, was sent in the span. Whether it should be a message the contact sent is open (#726).
- **Conversations**: one of the conversation's messages was sent in the span.
- **Messages**: the message was sent in the span.

### `first-message:`

Date. No value when the conversation has no message that is not a duplicate.

- **Conversations**: the conversation's first message was sent in the span.
- **Messages**: the first message of the message's conversation was sent in the span.

### `last-message:`

Date. No value when the conversation has no message that is not a duplicate.

- **Conversations**: the conversation's last message was sent in the span.
- **Messages**: the last message of the message's conversation was sent in the span.

### `first-heard:`

Date. No value when the contact never sent a message.

- **Contacts**: the first message the contact sent was sent in the span. A message the contact sent is one whose sender identity is one of the contact's, in any conversation, direct or group, that is not in the trash, duplicates left out. The account holder's own messages and other people's messages in a shared group do not count.

### `last-heard:`

Date. No value when the contact never sent a message.

- **Contacts**: the last message the contact sent, counted as for `first-heard:`, was sent in the span. The contact list's "Last heard from" column counts a conversation in the trash (#725).

### `attachment:`

Choice: `image`, `video`, `audio`, `document`, `pdf`, `contact`, `other`, `any`, `none`. Read from the MIME type.

- **Conversations**: one of the conversation's messages has an attachment of this kind. `none` is one with a message that has no attachment.
- **Messages**: the message has an attachment of this kind. `none` is a message with no attachment.

`image`, `video`, and `audio` are their MIME families. `pdf` is
`application/pdf`. `contact` is a vCard. `document` is a PDF, any `text/`
type that is not a vCard, any `application/vnd.` type, Word, or RTF. `other`
is anything else.

### `filename:`

Text, with no `none` or `any`.

- **Conversations**: one of the conversation's messages has an attachment whose file name contains the text.
- **Messages**: the message has one.

### `size:`

Size.

- **Conversations**: one of the conversation's messages has an attachment of this size.
- **Messages**: the message has one. An attachment with no recorded size matches no size.

### `messages:`

Count.

- **Contacts**: how many messages the contact's conversations hold, by anyone, conversations in the trash left out, so the count agrees with the contact drawer (#328). Whether it should count the messages the contact sent is open (#726).
- **Conversations**: how many messages the conversation holds.

### `conversations:`

Count.

- **Contacts**: how many conversations the contact is in.

### `groups:`

Count.

- **Contacts**: how many Contact Groups the contact is a member of. Unknown is computed, not a membership, and is not counted.

### `participants:`

Count.

- **Conversations**: how many participants the conversation has, a participant with no identity included and the account holder never.
- **Messages**: the same, for the message's conversation.

### `attachments:`

Count.

- **Messages**: how many attachments the message has.

### `trashed:`

Flag: `yes`, `no`, `any`. Any use lifts the list's trash default.

- **Contacts**: the contact is in the trash. `any` is every contact.
- **Conversations**: the conversation is in the trash.
- **Messages**: the message's conversation is in the trash.

## Adding a word

A word is one entry in `FIELDS` (`fields.rs`) and one arm in `emit.rs`. Its
entry here states the question it answers on each list before its SQL is
written, and the rules above are the review: one concept, the same question on
every list it is on, and `q` and `-q` splitting every list. The test
`every_word_compiles_and_runs_on_every_list_it_claims` checks the split on the
seeded vault for every word, value shape, and list, so a new word inherits it.
The user guide's word table and this section must name the new word on the
same lists, or the `docs` tests fail.

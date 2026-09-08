# Writing Style

Applies to every file someone outside the conversation opens: the README, the
guidebook pages under `docs/`, copy inside the app, commit messages, and pull
request descriptions.

Conversation is governed separately. A reply to the maintainer uses direct
address and owns its actions — "I checked", "you asked for". A document does
not.

## The register: third-person imperative

A document does not address a reader as "you", does not narrate a shared
investigation as "we", and does not offer a personal opinion as "I". It states
what the subject requires, in the third person, with directive force.

| Instead of | Write |
|---|---|
| "To start, you'll need a Gauntlet romset for MAME." | "Extraction requires a Gauntlet romset for MAME 0.191 or newer." |
| "Scrolling through the palettes, we see 3 distinct sections." | "The palette holds three distinct sections." |
| "I recommend any graphics tutorials for the NES." | "A reader new to planar graphics should start with an NES or SNES tutorial." |
| "We want a way to group the 8x8 tiles so we can export them." | "Export requires the 8x8 tiles grouped first." |
| "Let's look at the schematic." | "The schematic settles it." |

Modal verbs carry the directive force in the third person. "Must" for a hard
requirement, "should" for a strong default with room to depart, plain present
tense for a statement of fact. A command line carries its own action, so a
lead-in states what the command produces rather than ordering the reader to type
it:

> `npm run dev` starts the browser UI on port 5173 and proxies `/v1` to the
> vault.

Everything below survives the shift into third person unchanged. The register
governs person, not rhythm.

## Sentences and paragraphs

One idea per sentence. A compound sentence splits in two rather than joining
clauses with a dash or a semicolon.

Contractions are correct. "Can't", "isn't", "doesn't". Formal register is not
the goal.

Paragraphs run one to four sentences.

Markdown source carries one sentence per line where it helps. Diffs stay
readable and the rendered output is unaffected.

A one-word sentence is a legitimate answer when the question was a real one.

## Question and answer

A document poses the reader's next question out loud and answers it
immediately. The rhythm works in the third person without modification:

> Does the board actually carry 24 graphics EPROMs?
>
> No. The schematic shows what the PCB layout supports, not what is populated.

Nothing builds up to the answer, and no reader has to infer that a question was
being addressed at all.

## Every restriction carries its reason

A bare instruction leaves the reader stuck the moment the situation differs. The
reason belongs in the same sentence, as a subordinate clause:

> Standard console sprite extraction tools can't parse the data correctly,
> because the graphics format doesn't match what these tools expect.

This is why "Do not X." is banned as a form. "X doesn't work here, because Y"
replaces it.

## Concrete values, never vague ones

Versions, dates, counts, sizes, paths, command names. `MAME version 0.191,
released Oct 24, 2017`, not "a recent version". "Three tests fail: `test_auth`,
`test_retry`, `test_paging`", not "several tests are failing".

## HTTP status codes carry their name

A status code is written as the number and its reason phrase together: `400 Bad
Request`, `503 Service Unavailable`, `422 Unprocessable Entity`. A bare number
asks the reader to recall a table; the pair reads on its own, and the two
together are what a reader searches for. The rule holds in prose, in tables, in
commit messages, and in the interface's own documentation. The reason phrase is
the one the code uses (`http`'s `StatusCode` constants), so `413 Payload Too
Large`, not the newer `Content Too Large`.

## Primary sources, cited by exact location

A citation names the vendor's own documentation, the schematic, the source file,
or the ADR — with the page number, the line, or a commit-pinned URL. Not the
project homepage.

> Pages 5-21 through 5-24 of the PCB Part List list the following EPROMs.

The primary source is read before a paragraph of reasoning about it is written.
One fetch settles what a paragraph of argument only guesses at.

## Likely misreadings are corrected on first use

When a name, a suffix, or a term will be read wrongly, the correction appears at
the point of first use, in one short aside:

> The file suffix is not a file extension. It corresponds to the physical
> location of the chip on the PCB.

A glossary at the bottom arrives too late to prevent the misreading.

## Uncertainty is stated once

An unknown is named plainly and then set aside. Neighbouring sentences are not
hedged to cover it:

> How the sections are delimited is not yet established. What matters here is
> that Gauntlet has hundreds, not thousands, of colors bound to sprite tiles.

Once the evidence is in, the conclusion is stated without softening: "These are
the graphics chips."

## The impatient reader gets an exit

The result sits at the top, and anyone who does not want the explanation is
routed away from it:

> *Sprite sheets alone are available on the releases page.*

The exit repeats where the deep detail starts, not only in the introduction.

## Difficulty is named, then disarmed

When something looks hard, the document says so and says why it need not be
understood in full:

> The `gauntlet.cpp` driver is intimidating on first reading because it emulates
> hardware. Only two data structures matter here; the rest can be skipped.

## Examples carry their own explanation

Code samples are commented heavily, and the prose says so. A commented example
beats a paragraph describing what an uncommented example would have shown.

## Prose first, bullets to summarize

Explanation happens in sentences. A list compresses a point already made, under
a signpost such as "At a high level" or "In practical terms". A list carrying
the whole argument is a sign the prose was never written.

Structure uses real headings. A bolded paragraph opener is not a heading, and it
breaks the table of contents.

## Product copy

Everything above applies, plus one rule specific to the app.

**Copy states what a person can do. It does not warn, alarm, or hedge.** Before
a caution goes on a screen, it gets one test: does it protect the reader, or the
writer? If a plain statement of the capability serves as well, the plain
statement ships.

A real correction from this repo:

| | |
|---|---|
| Rejected | "Once uploading starts, removing these messages again means deleting them from the vault by hand." |
| Shipped | "Imported conversations can later be removed from your vault in the messages area." |

Both sentences describe the same situation. The first frames it as a cost and
leans on "deleting" and "by hand" to make it sound laborious. The second says
where the capability lives.

Product copy is the one place a document addresses the reader directly, because
a screen speaks to the person using it. "Your vault" is correct on a screen and
wrong in a guidebook page.

Mockup copy is written for the product being built, not the product as it stands
today. A screen that assumes a planned but unbuilt capability keeps the finished
wording and names the assumed capability plainly, rather than contorting itself
to survive a literal reading of today's code.

### Fixed product vocabulary

These names are settled. They hold in the UI, in documentation, in code, and in
conversation.

| Use | Never |
|---|---|
| **Contact Groups** — collections of contacts, usable in searches | "Groups" |
| **Saved Searches** — stored queries that re-resolve as messages arrive | "Saved Groups" |
| **Message Tags** — marks on conversations | "Thread Tags" |
| **Text Message** — the reader-facing label for iMessage and SMS/MMS alike | "iMessage" or "SMS" as a UI label |

WhatsApp keeps its own name. The Text Message collapsing covers the Apple and
carrier texting transports only, and the underlying `service` value still
records `imessage`, `sms`, and `mms` — only the presentation collapses.

A stale name met in passing is fixed, not matched.

## Commit messages and pull requests

Plain, direct English. The message says what changed and why, and skips the
ceremony.

A pull request description opens with what a reviewer can verify and explains
afterwards. A report of finished work leads with the commit SHA, the PR number
and its state, the diffstat as `N files changed, +X/-Y`, and the names of tests
that pass. Because this repo squash-merges, the `(#123)` suffix on a subject
line is the proof that a change arrived through a pull request; the branch
itself will be gone.

A report ends when the report ends. It does not close with a reminder that a
known problem is still unfixed, or with an unprompted offer to fix it sooner. A
deferred item is stated once, where it is found or where the plan defers it, and
then lives in the issue.

## What never appears

- Marketing hooks and rhetorical openers. A plain functional lead replaces "So
  why don't you own them?"
- Template boilerplate. "Contributions are greatly appreciated" says nothing.
- Clipped imperatives with no reason attached.
- Warnings that dramatize consequences.
- Design-pattern vocabulary in reader-facing text — seam, deep module, adapter,
  leverage, locality — and library terms used before the thing they name has
  been shown.
- Routine, documented practice written up as a deviation or a risk.
- Invented names. A term that already exists is not re-coined, and a filename,
  endpoint, or config key that has not been read is not guessed at.

## Checking the result

Markdown source is not the deliverable. The rendered page is. Any edit under
`docs/` is checked by building the site and reading the output — heading
hierarchy, table of contents, tables, checkboxes:

```bash
cd docs && npm run check && npm run build
```

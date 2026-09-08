# The HTTP interface has one rules document

The rules for the vault's `/v1` interface live in one standing document,
`docs/agents/http-api-rules.md`. That file states what the interface is today:
every rule, its reason, and the alternatives turned down. A pull request that
touches a route is graded against it, and a change to a rule is made there in
the same pull request that changes the code. No ADR is written for a decision
about the HTTP interface; this one exists only to say where those decisions
are.

## Why

By September 2026 the interface's rules were spread across four ADRs, each
amending a sentence in the one before. ADR-0003 settled identifiers, ADR-0005
settled response shape and then had its error body replaced by ADR-0010, and
ADR-0009 settled naming while correcting ADR-0005's singleton examples. A
reader checking whether one route was right had to read all four and apply the
amendments in their head, and the audit that found twenty-five conformance
problems graded them against "ADR-0003, ADR-0005 and standard REST practice",
which is what it looks like when there is no single document to grade against.

An ADR records one decision on one date and should not be edited into a
rulebook. An interface that changes freely before the first stable release
needs the opposite: a document that is always current and says nothing about
what the interface used to be. So the four ADRs were retired and their rules
moved into the rules document, each with a one-sentence reason and the
rejected alternatives that still matter. The dated narrative of what the
interface looked like in September is in git history and needs no other home.

## Considered and rejected

**Amending the ADRs in place.** Each would then read as if it had always said
the new thing, and the record of why the rules changed would be gone.

**A fifth ADR recording the session's decisions.** It would have been the
fifth document a reader had to merge by hand.

## Consequences

- ADR-0003, ADR-0005, ADR-0009 and ADR-0010 are deleted, and the closed audit
  `docs/agents/http-api-audit.md` with them. Their numbers are not reused.
- A decision about the HTTP interface is a rule in
  `docs/agents/http-api-rules.md`, not an ADR. ADRs continue for every other
  kind of decision.
- The document is what every future audit grades against.

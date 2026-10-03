---
name: pr-review
description: "Review a pull request on GitHub, fix what the review finds, and queue it for merging. Use to review a PR, or a branch that has one."
---

Review pull request `<N>` and take it to the merge queue. The PR is the record:
every **finding** is posted on it and answered there.

Read `AGENTS.md`, "Submitting Work" → "Review on the pull request" and
"Merging", before step 1. They hold the marker that every comment you post
starts with, the difference between an agent thread and a user thread, the
command for each step, and when a PR is queued. The steps below name the
AGENTS.md step they run.

## Closing a finding

Every finding closes one of three ways, with a reply in its thread saying which:

- **Fixed**: `Fixed in <sha>: <what changed>.`
- **Declined**: the finding is wrong or does not apply. The reply cites the
  evidence: the code, a test, or a repo document. Preference is not evidence.
- **Deferred**: the fix is real but outside this PR's scope or too large for it.
  File a GitHub issue (`docs/agents/issue-tracker.md`) and link it in the reply.

Then resolve the thread if it is an agent thread. A user thread gets the same
reply and the fix, and stays open for the user.

A judgement call is closed the same way as a hard violation. A small finding is
fixed, never declined for being small.

## Pushing and posting

The PR is a draft until step 5. Before CI, it is pushed at most twice: in
step 1, to bring it up to date, and in step 5, with every fix. Step 5 marks it
ready after its push, and CI is watched on that head alone (AGENTS.md
step 6).

**Every call that posts** (a review, a comment, a reply) goes one at a time,
and a refusal is retried as AGENTS.md says before its step 1.

**Before every push**, run the local checks (AGENTS.md step 3).

**After every push**, wait until GitHub reports the PR head as the commit you
pushed (the wait in AGENTS.md step 6) before you read the diff, post a review
pinned to it, or watch its checks. A finding's line is a line of the file at
the commit the review is pinned to: re-read the file there and move each line
that later commits shifted. A finding whose line can't be found there, or is
outside the PR diff at that commit, goes in a marked top-level comment
(AGENTS.md step 2).

**Another session's commits.** The PR is queued only while every commit on it
was reviewed by this run. When another session pushes to the branch, whether
your push is rejected or the PR head moves past your push, keep its commits:
merge them in (AGENTS.md step 5), never force over them. Then finish closing
the findings, and stop before queueing: report the PR and those commits to
the user.

**Merge review.** Every merge commit you make that resolves a conflict, in
any step, is reviewed before it is pushed. Resolve each conflict so both
sides' intent survives (the `resolving-merge-conflicts` skill), then spawn a
Correctness sub-agent with its step 2 brief and output rule, scoped to the
remerge diff of the merge commit (AGENTS.md step 5), so it reviews the
resolution alone. Fix its findings before the push. After the push, post them
in one review pinned to the pushed head, each answered `Fixed in <sha>` and
resolved, unless step 5 posts them with the re-review.

## Process

### 1. Pin the PR and bring it up to date

Take `<N>` from the argument, or from the PR for the current branch. With no
PR, stop and say to open one ("Submitting Work").

Make the PR a draft if it is not one (AGENTS.md step 1). Make the detached
worktree at the PR head (AGENTS.md step 3) and merge the base into it
whenever the PR is behind, whether or not it conflicts (AGENTS.md step 5). A
conflict gets the merge review. Push the merge: the review must be pinned to
a commit GitHub has. A PR already up to date is not pushed.

Then gather, once (AGENTS.md step 1):

- The diff, and the head SHA you review: the **reviewed head**, which is the
  merge you pushed, or the PR head when there was nothing to push.
- **The spec**: the issues the PR closes, plus any `#123` in its body or
  commits. With none, the Spec review is skipped and the summary says so.
- **The standards**: always `CLAUDE.md`, `AGENTS.md`,
  `docs/agents/writing-style.md` and `CONTEXT.md`. Add each
  `docs/architecture/` doc and `docs/adr/` record whose area the diff touches.
  `CLAUDE.md` names the doc for each area (contacts and identities, `/v1`
  routes, data fetching in `web/`, and so on).
- **Open threads** already on the PR, each marked agent or user.

Done when the reviewed head is the PR head on GitHub and includes the base,
and you hold the diff, the spec or its absence, the standards files, and the
open threads.

### 2. Review in parallel

Spawn three sub-agents in one message, so each reads the diff with fresh
context. Give each the PR number, how to get the diff, and its brief. Every
brief ends with the same output rule:

> Return a list of findings. Each finding: `path`, `line` (a line number in
> the file on the new side of the diff, or `none`), `kind` (`hard` or
> `judgement`), and the text: what is wrong, why it matters, and the fix.
> Return an empty list if you find nothing. Under 400 words.

- **Standards**: the standards files from step 1, and
  `.claude/skills/pr-review/smells.md`. "Report every place the diff breaks a
  documented repo rule, citing the file and the rule (`hard`), and every smell
  from the baseline you see, quoting the hunk (`judgement`). A repo rule
  overrides the baseline. Skip what tooling enforces."
- **Spec**: the issue text. "Report requirements the spec asked for that are
  missing or partial, behaviour the spec did not ask for, and requirements that
  look implemented but wrong. Quote the spec line for each."
- **Correctness**: "Find inputs or states that make this change produce a wrong
  result, crash, or lose data. Each finding names the concrete failing scenario:
  the input or state, and the wrong outcome. A worry with no scenario is not a
  finding." Correctness findings are `hard`.

Keep the axes separate, and post each axis's findings as its sub-agent
returned them, so one axis cannot mask another.

Post all findings in one review pinned to the reviewed head (AGENTS.md
step 2). Each line comment starts with the marker, then
`**<Axis> · <hard|judgement>**`, then the finding. A finding with `line: none`,
or whose line is outside the diff, goes in one top-level comment instead.

Done when every finding is on the PR.

### 3. Fix, without pushing

Commit a fix in the worktree for every finding from step 2 and every open
thread from step 1 that will be closed Fixed (see _Closing a finding_). Keep
the commits local until step 5.

Done when every finding and open thread is either fixed in a local commit or
has its Declined or Deferred reason ready.

### 4. Re-review the fixes

Spawn Standards and Correctness once more, in one message, on the local fix
commits only: give them the SHAs to read with `git show`. They are fresh
sub-agents and see the commits, never your reasoning for them. Fix what they
find in more local commits. This is the last review of the PR's own changes.

Done when every re-review finding is fixed or has its reason ready.

### 5. Push once

Fetch the base. If it moved since step 1, merge it in (a conflict gets the
merge review). Run the local checks, push, then mark the PR ready
(AGENTS.md step 6). With nothing to push, marking it ready starts CI.

After the push, close everything on the PR, using the pushed SHAs:

- Post the re-review findings, and any merge review findings, in one review
  pinned to the pushed head.
- Reply in every thread, and close it (_Closing a finding_, AGENTS.md
  step 4).

Done when the PR is ready, the push is accepted, every agent thread is
resolved, and every user thread has a reply.

### 6. Green CI

Watch the CI run that marking the PR ready started, stopping at its first
failed job (AGENTS.md step 6). A job that fails because of the PR is a finding: fix it,
run the local checks, push, and watch the new run.

A check that fails for a reason outside the PR (a red `main`, a runner fault,
a network fetch) gets one rerun of its failed jobs. If it fails again, stop
and report it without changing the code for it.

A failed check is sorted, and the run rerun, as AGENTS.md step 6 says.

Done when the CI run on the commit you pushed ended in `success`, and it is
still the PR head (_Another session's commits_).

### 7. Summarise and queue

Check the PR against its base once more (AGENTS.md step 5). On `CONFLICTING`,
merge the base (with the merge review), push, and return to step 6. A PR that
is only behind is queued as it is: the queue tests it on the latest base.

Post one top-level comment, starting with the marker:

- Findings per axis, and how many were Fixed, Declined, and Deferred, with
  the deferred issues linked.
- Commits made for CI failures, and each merge of the base with the files
  whose conflicts it resolved.
- Any Spec skip, and any user thread still open.

Queue the PR on the commit you pushed when "Merging" says it is ready. Otherwise, say in the summary and to the user
what it waits on, such as an open user thread.

Remove the worktree. Report to the user: the PR, the counts, and whether it is
queued.

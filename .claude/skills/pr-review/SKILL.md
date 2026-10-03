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

## Process

### 1. Pin the PR

Take `<N>` from the argument, or from the PR for the current branch. With no
PR, stop and say to open one ("Submitting Work").

Gather, once (AGENTS.md step 1):

- The diff, and the head SHA you review: the **reviewed head**.
- **The spec**: the issues the PR closes, plus any `#123` in its body or
  commits. With none, the Spec review is skipped and the summary says so.
- **The standards**: always `CLAUDE.md`, `AGENTS.md`,
  `docs/agents/writing-style.md` and `CONTEXT.md`. Add each
  `docs/architecture/` doc and `docs/adr/` record whose area the diff touches.
  `CLAUDE.md` names the doc for each area (contacts and identities, `/v1`
  routes, data fetching in `web/`, and so on).
- **Open threads** already on the PR, each marked agent or user.

Done when you hold the diff, the reviewed head, the spec or its absence, the
standards files, and the open threads.

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

### 3. Post

Post all findings in one review pinned to the reviewed head (AGENTS.md
step 2). Each line comment starts with the marker, then
`**<Axis> · <hard|judgement>**`, then the finding. A finding with `line: none`,
or whose line is outside the diff, goes in one top-level comment instead.

Done when every finding from step 2 is on the PR.

### 4. Fix

Work in a detached worktree at the reviewed head, and push as AGENTS.md
step 3 says. Close every finding from step 3 and every open thread from
step 1 (see _Closing a finding_, and AGENTS.md step 4). Run
`./scripts/check-pr.sh` before each push. Keep the SHA of every fix commit.

Done when every agent thread is resolved and every user thread has a reply.

### 5. Re-review the fixes

Run Standards and Correctness once more, on the fix commits only: give them
the fix commit SHAs to read with `git show`. A rebase in step 4 changes those
SHAs, so use the ones that were pushed. Post and close their findings as in
steps 3 and 4. This is the last review of the PR's own changes.

### 6. Resolve conflicts with the base

Check whether the PR conflicts with its base (AGENTS.md step 5). If it does,
merge the base into the worktree and resolve each conflict so both sides'
intent survives (the `resolving-merge-conflicts` skill). Run
`./scripts/check-pr.sh`. Then spawn a Correctness sub-agent with its step 2
brief and output rule, scoped to the remerge diff of the merge commit
(AGENTS.md step 5), so it reviews the resolution alone. Close its findings as
in steps 3 and 4, and push.

Done when GitHub reports the PR `MERGEABLE`.

### 7. Green CI

Wait for the required checks (AGENTS.md step 6). A check that fails because of
the PR is a finding. Fix it in the worktree, push, and wait again. A check that
fails for a reason outside the PR (a red `main`, a runner fault, a network
fetch) gets one rerun of its failed jobs. If it fails again, stop and report
it without changing the code for it.

### 8. Summarise and queue

Post one top-level comment, starting with the marker:

- Findings per axis, and how many were Fixed, Declined, and Deferred, with
  the deferred issues linked.
- Commits made for CI failures, and any merge of the base with the files
  whose conflicts it resolved.
- Any Spec skip, and any user thread still open.

Check the PR against its base once more (AGENTS.md step 5), because `main`
may have moved while CI ran. On `CONFLICTING`, go back to step 6. Queue the
PR when it is `MERGEABLE` and "Merging" says it is ready. Otherwise, say in the summary
and to the user what it waits on, such as an open user thread.

Remove the worktree. Report to the user: the PR, the counts, and whether it is
queued.

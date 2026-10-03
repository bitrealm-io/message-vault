---
name: pr-review
description: "Review a pull request on GitHub and see it through to the merge queue: three parallel reviews (Standards, Spec, Correctness) post every finding on the PR, then each finding is fixed, declined, or deferred, its thread answered and resolved, CI made green, and the PR queued. Use to review a PR, or a branch that has one, before it merges."
---

Review pull request `<N>` on GitHub and take it to the merge queue. The PR is the
record: every **finding** is posted on it, every thread is answered there, and
nothing the review raised lives only in chat.

The `gh` commands for every step are in `AGENTS.md`, "Submitting Work" →
"Review on the pull request" and "Merging". Read that section before step 3.
Why the gate works this way: `docs/adr/0007-ci-is-the-only-gate.md`.

## The marker

Every comment and review this skill posts starts with the line
`<!-- pr-review -->`. You and every agent post from the same GitHub account, so
the marker is the only way to tell an agent's thread from the user's. A thread
whose first comment carries it is an **agent thread**; any other thread is a
**user thread**.

## Closing a finding

Every finding closes one of three ways, with a reply in its thread saying which:

- **Fixed**: `Fixed in <sha>: <what changed>.`
- **Declined**: the finding is wrong or does not apply. The reply cites the
  evidence: the code, a test, or a repo document. Preference is not evidence.
- **Deferred**: the fix is real but outside this PR's scope or too large for it.
  File a GitHub issue (`docs/agents/issue-tracker.md`) and link it in the reply.

Then resolve the thread if it is an agent thread. A user thread gets the same
reply and the fix, and stays open: resolving it is the user's call.

A judgement call is closed the same way as a hard violation. A small finding is
fixed, never declined for being small.

## Process

### 1. Pin the PR

Take `<N>` from the argument, or `gh pr view --json number` for the current
branch. No PR → stop and say to open one ("Submitting Work").

Gather, once:

- The diff: `gh pr diff <N>`, and the head SHA and base branch from
  `gh pr view <N> --json headRefName,headRefOid,baseRefName,isDraft,closingIssuesReferences,body`.
- **The spec**: the issues the PR closes, plus any `#123` in its body or
  commits. None → the Spec review is skipped, and the summary says so.
- **The standards**: always `CLAUDE.md`, `AGENTS.md`,
  `docs/agents/writing-style.md` and `CONTEXT.md`. Add each
  `docs/architecture/` doc and `docs/adr/` record whose area the diff touches;
  `CLAUDE.md` names the doc for each area (contacts and identities, `/v1`
  routes, data fetching in `web/`, and so on).
- **Open threads** already on the PR, each marked agent or user.

Done when you hold the diff, the spec (or its absence), the list of standards
files, and the open threads.

### 2. Review in parallel

Spawn three sub-agents in one message, so each reads the diff with fresh
context. Give each the PR number, the diff command, and its brief. Every brief
ends with the same output rule:

> Return a list of findings. Each finding: `path`, `line` (a line on the new
> side of the diff, or `none`), `kind` (`hard` or `judgement`), and the text:
> what is wrong, why it matters, and the fix. Return an empty list if you find
> nothing. Under 400 words.

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

Keep the axes separate: do not merge, rerank, or drop findings across them.

### 3. Post

Post all findings in one review on the PR. Each line comment starts with the
marker, then `**<Axis> · <hard|judgement>**`, then the finding. A finding with
`line: none`, or whose line is outside the diff, goes in one top-level PR
comment instead; it is answered the same way and has nothing to resolve.

Done when every finding from step 2 is on the PR.

### 4. Fix

Work in a fresh worktree at the PR's head:
`git worktree add .worktrees/review-<N> <headRefName>` after a fetch.

Close every finding from step 3 and every open thread from step 1 (see
_Closing a finding_). Before each push run `./scripts/check-pr.sh`. Push with a
plain `git push`. If the push is rejected because the branch moved, fetch,
rebase your fix commits onto the new head, rerun the checks, and push again.
Never force-push: the branch may carry another session's commits.

Done when every agent thread is resolved and every user thread has a reply.

### 5. Re-review the fixes

Run Standards and Correctness once more, on the fix commits only
(`git diff <head SHA from step 1>..HEAD`). Post and close their findings as in
steps 3 and 4. This is the last review round.

### 6. Green CI

Wait for the required checks (`gh pr checks <N> --watch --required`). A check
that fails because of the PR is a finding: fix it in the worktree, push, and
wait again. A check that fails for a reason outside the PR (a red `main`, a
runner fault, a network fetch) gets one rerun of the failed jobs; if it fails
again, stop and report it without changing the code for it.

### 7. Summarise and queue

Post one top-level comment, with the marker:

- Findings per axis, and how many were Fixed, Declined, and Deferred, with
  the deferred issues linked.
- Commits made for CI failures.
- Any Spec skip, and any user thread still open.

Then queue the PR with `gh pr merge <N>` when every thread is resolved, the
required checks are green, and the PR is not a draft. Otherwise leave it out of
the queue, and say in the summary and to the user what it waits on.

Remove the worktree. Report to the user: the PR, the counts, and whether it is
queued.

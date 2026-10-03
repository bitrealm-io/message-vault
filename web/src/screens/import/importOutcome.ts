import type { ImportIssue } from "../../components/import/ImportSummaryPanel";
import type {
  AttachmentForecast,
  PushFinishedReport,
  SizeVerdict,
  StagingSummary,
} from "../../lib/tauri";

/**
 * How an Upload ended. `paused` is an Upload that did not send every
 * conversation: the run stays open at its Upload with its staged folder,
 * and the next visit to Import offers Resume or Discard (CONTEXT.md,
 * "Pause").
 */
export type ImportOutcome = "completed" | "completed_with_issues" | "paused";

/**
 * The stable identity of a staged attachment across Media.
 *
 * A committed derivative changes name: `attachments/2024-01-15-9f2a3b4c.heic`
 * becomes `attachments/2024-01-15-9f2a3b4c-mv.jpg`. The stem gains a literal
 * `-mv` suffix and the extension changes, but the `{date}-{digest16}` stem in
 * front of it is stable (`attachment_dest_name`,
 * `crates/core/message-crate-core/src/attachments.rs`). Match on that, or
 * a converted file reads as a different file from the one that was approved.
 */
export function stableStem(path: string): string {
  const base = path.split("/").pop() ?? path;
  const dot = base.lastIndexOf(".");
  const stem = dot > 0 ? base.slice(0, dot) : base;
  return stem.endsWith("-mv") ? stem.slice(0, -3) : stem;
}

/**
 * Verdicts that predict a file will not make it into Message Crate at all
 * (spec decision 15). `likely_fits`/`may_grow`/`fits_as_is` predict the file
 * lands, so a skip for one of those was not on the approved plan.
 */
const OMITTABLE_VERDICTS: ReadonlySet<SizeVerdict> = new Set([
  "probably_too_big",
  "cannot_process",
]);

/**
 * Whether `item` — an issue's file identifier — names the same physical
 * attachment as `row`, an approved plan's forecast row.
 *
 * A push issue's `item` is `"{conversationFile}:{relativePath}"`
 * (the `AttachmentSkip` that `crates/libs/push/src/prepare.rs` builds), not a bare
 * path, so exact equality against `row.path`/`row.name` only catches the
 * simple case. `item.endsWith(...)` catches the compound form without the
 * conversation-name prefix tripping it up. `stableStem` (Task 9's helper)
 * catches a file the media pass renamed with a `-mv` suffix between the
 * review and the push — `stem.pop()` on the path already strips any
 * `{name}:` prefix, since it only looks at the last `/`-separated segment.
 */
function issueNamesForecastRow(item: string, row: AttachmentForecast): boolean {
  if (item === row.path || item === row.name) return true;
  if (item.endsWith(row.path) || item.endsWith(row.name)) return true;
  return stableStem(item) === stableStem(row.path);
}

/**
 * True when `approved` already flagged the file this issue is about as one
 * that would not make it in. Failures (`kind: "error"`) are never excused —
 * only a `skip`, which is what an expected omission actually looks like on
 * the wire.
 */
function isApprovedOmission(
  issue: Pick<ImportIssue, "kind" | "item">,
  approved: StagingSummary | undefined,
): boolean {
  if (!approved || issue.kind === "error" || !issue.item) return false;
  const item = issue.item;
  return approved.forecasts.some(
    (row) => OMITTABLE_VERDICTS.has(row.verdict) && issueNamesForecastRow(item, row),
  );
}

/**
 * Verdict for an Upload, read from the push report rather than from whether
 * the push call returned (spec decisions 21–22).
 *
 * An Upload finishes only when every conversation was sent or was already
 * sent: anything else is `paused`, never failed (#1233). That covers a push
 * the cancel flag stopped, one that threw or left no report, a conversation
 * the server did not take (a server that stops answering part-way fails
 * every later conversation and the push moves on), a conversation left
 * unsent, and an Upload that sent nothing at all. The push journal leaves
 * every such conversation for the next push, so resuming sends only what is
 * missing. Pausing on a failure keeps those conversations reachable: a run
 * recorded as finished would delete the folder they are staged in.
 *
 * A finished Upload with item-level problems is `completed_with_issues` —
 * unless `approved` already told the user about them: the plan the user
 * approved at their last gate (spec decision 15 — Gate 2's plan when there
 * was a media pass, Gate 1's otherwise) is diffed against the issues the
 * run reported, and a skip the plan already forecast is not news. Without
 * `approved` every issue counts.
 */
export function importOutcome(args: {
  report: PushFinishedReport | undefined;
  threw: boolean;
  issues: readonly ImportIssue[];
  approved?: StagingSummary;
}): ImportOutcome {
  const { report, threw, issues, approved } = args;
  if (threw || !report) return "paused";
  const everyConversationSent =
    !report.cancelled &&
    report.conversations_failed === 0 &&
    report.conversations_cancelled === 0 &&
    report.conversations_ok + report.conversations_skipped === report.conversations_total;
  if (!everyConversationSent) return "paused";
  const unexplained = issues.filter((issue) => !isApprovedOmission(issue, approved));
  if (report.messages_failed > 0 || unexplained.length > 0) return "completed_with_issues";
  return "completed";
}

import type { ImportIssue } from "../../components/import/ImportSummaryPanel";
import type {
  AttachmentForecast,
  PushFinishedReport,
  SizeVerdict,
  StagingSummary,
} from "../../lib/tauri";

export type ImportOutcome = "completed" | "completed_with_issues" | "failed";

/**
 * The stable identity of a staged attachment across Media.
 *
 * A committed derivative changes name: `attachments/2024-01-15-9f2a3b4c.heic`
 * becomes `attachments/2024-01-15-9f2a3b4c-mv.jpg`. The stem gains a literal
 * `-mv` suffix and the extension changes, but the `{date}-{digest16}` stem in
 * front of it is stable (`attachment_dest_name`,
 * `crates/core/message-vault-io-core/src/attachments.rs`). Match on that, or
 * a converted file reads as a different file from the one that was approved.
 */
export function stableStem(path: string): string {
  const base = path.split("/").pop() ?? path;
  const dot = base.lastIndexOf(".");
  const stem = dot > 0 ? base.slice(0, dot) : base;
  return stem.endsWith("-mv") ? stem.slice(0, -3) : stem;
}

/**
 * Verdicts that predict a file will not make it into the vault at all
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
 * (`crates/cli/vault-push/src/run.rs`'s `AttachmentSkipIssue`), not a bare
 * path, so exact equality against `row.path`/`row.name` only catches the
 * simple case. `item.endsWith(...)` catches the compound form without the
 * conversation-name prefix tripping it up. `stableStem` (Task 9's helper)
 * catches a file the media pass renamed with a `-mv` suffix between the
 * approval and the push — `stem.pop()` on the path already strips any
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
 * Three-way verdict for a finished import, read from the push report rather
 * than from whether the push call returned (spec decisions 21–22).
 *
 * `failed` has a zero floor: interrupted, threw, or nothing landed at all.
 * A re-push where every conversation dedupes to a skip is a no-op, not a
 * failure. Item-level problems make it `completed_with_issues` — unless
 * `approved` already told the user about them: the plan the user approved
 * at their last gate (spec decision 15 — Gate 2's plan when there was a
 * media pass, Gate 1's otherwise) is diffed against the issues the push
 * actually reported, and a skip the plan already forecast is not news.
 * `approved` is optional and its absence behaves exactly as before this
 * task — a resumed push has no stored plan to diff against.
 */
export function importOutcome(args: {
  report: PushFinishedReport | undefined;
  threw: boolean;
  issues: readonly ImportIssue[];
  approved?: StagingSummary;
}): ImportOutcome {
  const { report, threw, issues, approved } = args;
  if (threw || !report) return "failed";
  const nothingLanded =
    report.conversations_total > 0 &&
    report.conversations_ok === 0 &&
    report.conversations_skipped === 0;
  if (nothingLanded) return "failed";
  const unexplained = issues.filter((issue) => !isApprovedOmission(issue, approved));
  if (report.conversations_failed > 0 || report.messages_failed > 0 || unexplained.length > 0) {
    return "completed_with_issues";
  }
  return "completed";
}

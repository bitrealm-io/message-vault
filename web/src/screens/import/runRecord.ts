import type { ImportIssue } from "../../components/import/ImportSummaryPanel";
import { isIssueStage } from "../../components/import/importIssueStage";
import type { PushFinishedReport } from "../../lib/tauri";

/**
 * What an Import Run has recorded so far, across every part it ran in.
 *
 * A run runs in parts when it pauses, or stops at a Review, and resumes
 * later, perhaps after the app closed. The server takes the run's record
 * only with `/complete`, which a paused run never posts, so the window keeps
 * the record of the earlier parts in the staging folder
 * (`read_import_run_record`, `save_import_run_record`) and the completion
 * the last part posts covers the whole run: its Import Errors, timings,
 * bytes and counts.
 *
 * Durations and the Upload's counters add up across parts. The Staging
 * counts are the latest part's, since a resumed Staging reports on the
 * conversations it wrote itself.
 */
export type RunRecord = {
  issues: ImportIssue[];
  durationMs?: number;
  parseMs?: number;
  attachmentsMs?: number;
  prepareMs?: number;
  uploadMs?: number;
  bytesUploaded?: number;
  filesParsed?: number;
  messagesParsed?: number;
  /** Conversations an Upload sent whole. */
  filesSucceeded?: number;
  messagesAttempted?: number;
  messagesInserted?: number;
  messagesDeduped?: number;
  attachmentsUploaded?: number;
};

/** The record of a run with no earlier part. */
export const EMPTY_RUN_RECORD: RunRecord = { issues: [] };

const COUNT_FIELDS = [
  "durationMs",
  "parseMs",
  "attachmentsMs",
  "prepareMs",
  "uploadMs",
  "bytesUploaded",
  "filesParsed",
  "messagesParsed",
  "filesSucceeded",
  "messagesAttempted",
  "messagesInserted",
  "messagesDeduped",
  "attachmentsUploaded",
] as const;

function isIssue(value: unknown): value is ImportIssue {
  if (typeof value !== "object" || value === null) return false;
  const r = value as Record<string, unknown>;
  return (
    typeof r.kind === "string" &&
    isIssueStage(r.stage) &&
    typeof r.item === "string" &&
    typeof r.reason === "string"
  );
}

/**
 * Read a record back from the staging folder. The file is the app's own,
 * but it is read from disk, so each field is checked: an unreadable field
 * is left out, and anything that is not a record reads as no earlier part.
 */
export function parseRunRecord(raw: unknown): RunRecord {
  if (typeof raw !== "object" || raw === null) return EMPTY_RUN_RECORD;
  const r = raw as Record<string, unknown>;
  const record: RunRecord = {
    issues: Array.isArray(r.issues) ? r.issues.filter(isIssue) : [],
  };
  for (const field of COUNT_FIELDS) {
    const value = r[field];
    if (typeof value === "number" && Number.isFinite(value)) record[field] = value;
  }
  return record;
}

/** The part of the run on screen now, as the window measured it. */
export type RunPart = {
  issues: readonly ImportIssue[];
  durationMs: number;
  parseMs: number | null;
  attachmentsMs: number | null;
  prepareMs: number | null;
  uploadMs: number | null;
  filesParsed?: number;
  messagesParsed?: number;
  /** This part's push report, when it ran an Upload that reported. */
  report: PushFinishedReport | null;
};

/**
 * Whole milliseconds: the server stores durations as integers and refuses a
 * fraction, and `performance.now()` differences carry one.
 */
function sum(a: number | undefined, b: number | null | undefined): number | undefined {
  if (a == null && b == null) return undefined;
  return Math.round((a ?? 0) + (b ?? 0));
}

/** Conversation files of `report` whose result has one of `statuses`. */
function conversationFiles(
  report: PushFinishedReport | null,
  statuses: readonly string[],
): Set<string> {
  return new Set(
    (report?.results ?? [])
      .filter((result) => statuses.includes(result.status))
      .map((result) => result.file),
  );
}

/**
 * The whole run so far: the earlier parts' record with this part added.
 *
 * An Upload's `skipped` row for a conversation is left out. The push
 * journal skips only conversations an earlier part of this same run sent,
 * since every run stages into a folder of its own, so the row says nothing
 * the run's counts do not.
 */
export function wholeRun(carried: RunRecord, part: RunPart): RunRecord {
  const report = part.report;
  const skipped = conversationFiles(report, ["skipped"]);
  return {
    issues: [
      ...carried.issues,
      ...part.issues.filter((issue) => !(issue.stage === "upload" && skipped.has(issue.item))),
    ],
    durationMs: sum(carried.durationMs, part.durationMs),
    parseMs: sum(carried.parseMs, part.parseMs),
    attachmentsMs: sum(carried.attachmentsMs, part.attachmentsMs),
    prepareMs: sum(carried.prepareMs, part.prepareMs),
    uploadMs: sum(carried.uploadMs, part.uploadMs),
    bytesUploaded: sum(carried.bytesUploaded, report?.assets_bytes),
    filesParsed: part.filesParsed ?? carried.filesParsed,
    messagesParsed: part.messagesParsed ?? carried.messagesParsed,
    filesSucceeded: sum(carried.filesSucceeded, report?.conversations_ok),
    messagesAttempted: sum(carried.messagesAttempted, report?.messages_attempted),
    messagesInserted: sum(carried.messagesInserted, report?.messages_inserted),
    messagesDeduped: sum(carried.messagesDeduped, report?.messages_deduped),
    attachmentsUploaded: sum(carried.attachmentsUploaded, report?.assets_uploaded),
  };
}

/** The `item` of the error a stage records about the run as a whole. */
export const RUN_ERROR_ITEM = "Import";

/**
 * The record a stopped part leaves for the next one.
 *
 * Two kinds of issue are left out of it. An Upload's row for a whole
 * conversation (failed, skipped, or left unsent) is left out because the
 * resumed Upload sends or skips that conversation again and reports it
 * afresh. The run-level error a stage records when it stops (item `Import`)
 * is left out because it explains the stop, not the run. An Upload's
 * attachment skips are kept, because the resumed Upload does not read those
 * conversations again.
 */
export function recordToCarry(carried: RunRecord, part: RunPart): RunRecord {
  const conversations = conversationFiles(part.report, ["failed", "skipped", "cancelled"]);
  const issues = part.issues.filter(
    (issue) =>
      !(issue.kind === "error" && issue.item === RUN_ERROR_ITEM) &&
      !(issue.stage === "upload" && conversations.has(issue.item)),
  );
  return wholeRun(carried, { ...part, issues });
}

/**
 * Conversations the run found already imported. A resumed Upload's report
 * counts the conversations an earlier part sent as skipped, since the push
 * journal lists them, so those are taken back out.
 */
export function filesSkippedOverRun(carried: RunRecord, report: PushFinishedReport): number {
  return Math.max(0, report.conversations_skipped - (carried.filesSucceeded ?? 0));
}

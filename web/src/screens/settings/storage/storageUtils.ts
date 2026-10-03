import type { ImportSummaryView } from "../../../components/import/ImportSummaryPanel";
import { formatDateTime } from "../../../lib/formatDate";
import type { components } from "../../../lib/serverApi.types";

export const ATTACHMENT_PAGE_SIZE = 20;

/** Import Runs or Export Runs on one page of a history table, and the `limit` each request sends. */
export const RUN_PAGE_SIZE = 50;

export const sectionTitle = "m-0 text-[0.938rem] font-semibold text-text";
export const sectionHint = "mt-1 text-[0.813rem] text-muted";
export const tableCard = "overflow-hidden rounded-lg";
export const thStyle =
  "border-b border-border bg-elevated p-2 px-3 text-left text-[0.813rem] font-medium text-muted";
export const tdStyle = "border-b border-border p-2 px-3 text-[0.813rem] text-text";

/*
 * These three shapes come from the server, so they are generated rather than
 * written here: a field renamed on the server is a build error instead of a
 * blank cell in the storage table.
 */
type Schema = components["schemas"];

/**
 * One past Import Run, as the imports list returns it: in full to the
 * account itself, and to the owner without what the run held.
 */
export type ListedImportRun = Schema["ImportRun"] | Schema["OwnerImportRun"];

/**
 * One Export Run as the history table lists it: in full to the account
 * itself, and to the owner without what the run asked for.
 */
export type ExportRow = Schema["ExportRun"] | Schema["OwnerExportRun"];

/** One large attachment in the storage breakdown. */
export type TopAttachment = Schema["TopAttachment"];

/**
 * What an Export Run asked for, in one line. The account reads its own runs
 * in full ({@link describeExportScope}); the owner is told only which of the
 * three forms the scope took, because the search is the account's own.
 */
export function describeExportRun(run: ExportRow): string {
  if (!("scope_kind" in run)) return describeExportScope(run.scope);
  switch (run.scope_kind) {
    case "everything":
      return "Everything";
    case "query":
      return "A search";
    case "selection":
      return "Picked by hand";
    default:
      run.scope_kind satisfies never;
      return run.scope_kind;
  }
}

/**
 * What an Export Run asked for, in one line: "Everything", the search it
 * ran and the list it ran on, or how many conversations and messages were
 * picked by hand.
 */
export function describeExportScope(scope: Schema["ExportScope"]): string {
  switch (scope.kind) {
    case "everything":
      return "Everything";
    case "query":
      return scope.list === "conversations"
        ? `Conversations found by: ${scope.q}`
        : `Messages found by: ${scope.q}`;
    case "selection": {
      const parts: string[] = [];
      const conversations = scope.conversation_ids?.length ?? 0;
      const messages = scope.message_ids?.length ?? 0;
      if (conversations > 0) {
        parts.push(`${conversations} conversation${conversations === 1 ? "" : "s"}`);
      }
      if (messages > 0) {
        parts.push(`${messages} message${messages === 1 ? "" : "s"}`);
      }
      return `Picked: ${parts.join(", ")}`;
    }
  }
}

/**
 * One Import Run: in full, with its summary and issues, to the account
 * itself; to the owner with the summary's counts and how many issues.
 */
export type AccountImportRun = Schema["AccountImportRun"];

/** Human-readable file size (for example "1.2 MB"). */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = value >= 10 || unit === 0 ? 0 : 1;
  return `${value.toFixed(digits)} ${units[unit]}`;
}

/** A count with its noun, pluralised by adding `s`: "1 message", "1,234 messages". */
export function countOf(n: number, noun: string): string {
  return `${n.toLocaleString()} ${noun}${n === 1 ? "" : "s"}`;
}

/** Import start/finish time for table rows, or an em dash when missing. */
export function formatImportDate(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return formatDateTime(iso);
}

/** Finite number, or undefined for anything else. */
function toNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

/**
 * Map a server import status onto the summary panel's five statuses. Every
 * status is named: one the server adds fails the type-check at `satisfies
 * never` until it has a case here.
 */
function toSummaryStatus(status: Schema["ImportStatus"]): ImportSummaryView["status"] {
  switch (status) {
    case "completed":
      return "completed";
    case "completed_with_issues":
      return "completed_with_issues";
    case "cancelled":
      return "cancelled";
    case "running":
      return "running";
    case "failed":
      return "failed";
    default:
      status satisfies never;
      return "failed";
  }
}

/**
 * The word the import detail panel shows for a run's status. Every status is
 * named, as in `toSummaryStatus`.
 */
export function importStatusLabel(status: Schema["ImportStatus"]): string {
  switch (status) {
    case "running":
      return "Running";
    case "completed":
      return "Completed";
    case "completed_with_issues":
      return "Completed with issues";
    case "failed":
      return "Failed";
    case "cancelled":
      return "Cancelled";
    default:
      status satisfies never;
      return status;
  }
}

/** Build the import summary panel model from an Account Import Run. */
export function toImportSummaryView(detail: AccountImportRun): ImportSummaryView {
  // The owner reads the summary's counts and nothing else of it.
  const summary: Record<string, unknown> =
    "counts" in detail
      ? detail.counts
      : detail.summary && typeof detail.summary === "object"
        ? (detail.summary as Record<string, unknown>)
        : {};
  const hasAnyStageTiming =
    detail.parse_ms != null ||
    detail.attachments_ms != null ||
    detail.prepare_ms != null ||
    detail.upload_ms != null;
  const durationMs =
    detail.duration_ms ??
    (hasAnyStageTiming
      ? (detail.parse_ms ?? 0) +
        (detail.attachments_ms ?? 0) +
        (detail.prepare_ms ?? 0) +
        (detail.upload_ms ?? 0)
      : null);

  return {
    status: toSummaryStatus(detail.status),
    filesTotal: toNumber(summary.files_total ?? summary.filesTotal),
    filesSucceeded: toNumber(summary.files_succeeded ?? summary.filesSucceeded),
    filesFailed: toNumber(summary.files_failed ?? summary.filesFailed),
    filesSkipped: toNumber(summary.files_skipped ?? summary.filesSkipped),
    messagesParsed: toNumber(
      summary.messages_parsed ??
        summary.messagesParsed ??
        summary.parse_messages ??
        summary.parseMessages,
    ),
    messagesAttempted: toNumber(summary.messages_attempted ?? summary.messagesAttempted),
    messagesInserted:
      toNumber(summary.messages_inserted ?? summary.messagesInserted) ?? detail.message_count,
    messagesDeduped: toNumber(summary.messages_deduped ?? summary.messagesDeduped),
    messagesFailed: toNumber(summary.messages_failed ?? summary.messagesFailed),
    parseMs: detail.parse_ms,
    attachmentsMs: detail.attachments_ms,
    prepareMs: detail.prepare_ms,
    uploadMs: detail.upload_ms,
    durationMs,
    issues: "issues" in detail ? detail.issues : [],
  };
}

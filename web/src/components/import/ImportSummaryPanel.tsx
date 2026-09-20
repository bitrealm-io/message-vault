import StepProgress, { type Step, type StepStatus } from "../StepProgress";
import VirtualizedImportIssuesTable from "./VirtualizedImportIssuesTable";

export type ImportIssue = {
  kind: string;
  step: string;
  item: string;
  reason: string;
};

export type ImportSummaryView = {
  status: "completed" | "completed_with_issues" | "failed" | "canceled" | "running";
  filesTotal?: number;
  filesSucceeded?: number;
  filesFailed?: number;
  filesSkipped?: number;
  messagesParsed?: number;
  messagesAttempted?: number;
  messagesInserted?: number;
  messagesDeduped?: number;
  messagesFailed?: number;
  /** Attachment files the upload sent; known only to the run that did the upload. */
  attachmentsUploaded?: number;
  parseMs?: number | null;
  attachmentsMs?: number | null;
  prepareMs?: number | null;
  uploadMs?: number | null;
  durationMs: number | null;
  issues: ImportIssue[];
};

type ImportSummaryPanelProps = {
  summary: ImportSummaryView;
  /** When true (default), show Parse/Attachments/Prepare/Upload with times above the tables. */
  embedStepTimings?: boolean;
};

type MessageRow = {
  key: string;
  label: string;
  value: number | undefined;
  indent?: boolean;
};

function formatCount(value: number | undefined): string {
  if (value == null) return "—";
  return value.toLocaleString();
}

function difference(total: number | undefined, accounted: number | undefined): number | undefined {
  if (total == null || accounted == null) return undefined;
  return total - accounted;
}

export function completionTextFor(
  status: ImportSummaryView["status"] | undefined,
): string | undefined {
  if (status === "completed") return "Import complete";
  if (status === "completed_with_issues") return "Import completed with issues";
  if (status === "failed") return "Import failed";
  if (status === "canceled") return "Import canceled";
  return undefined;
}

function historySteps(summary: ImportSummaryView): Step[] {
  const running = summary.status === "running";

  let attachmentsStatus: StepStatus = "done";
  if (running) {
    attachmentsStatus = summary.parseMs != null ? "active" : "pending";
  }

  let prepareStatus: StepStatus = "done";
  if (running) {
    prepareStatus = summary.attachmentsMs != null ? "active" : "pending";
  }

  let uploadStatus: StepStatus = "done";
  if (summary.status === "failed") {
    uploadStatus = "error";
  } else if (running) {
    uploadStatus =
      summary.prepareMs != null || summary.attachmentsMs != null || summary.parseMs != null
        ? "active"
        : "pending";
  }

  return [
    {
      label: "Parse backup",
      status: running ? "active" : "done",
      durationMs: summary.parseMs,
    },
    {
      label: "Attachments",
      status: attachmentsStatus,
      durationMs: summary.attachmentsMs,
    },
    {
      label: "Preparing messages",
      status: prepareStatus,
      durationMs: summary.prepareMs,
    },
    {
      label: "Upload to vault",
      status: uploadStatus,
      durationMs: summary.uploadMs,
    },
  ];
}

export default function ImportSummaryPanel({
  summary,
  embedStepTimings = true,
}: ImportSummaryPanelProps) {
  const messagesSkipped = difference(summary.messagesParsed, summary.messagesAttempted);
  const attemptedAccounted =
    summary.messagesInserted != null &&
    summary.messagesDeduped != null &&
    summary.messagesFailed != null
      ? summary.messagesInserted + summary.messagesDeduped + summary.messagesFailed
      : undefined;
  const attemptMismatch =
    summary.messagesAttempted != null &&
    attemptedAccounted != null &&
    summary.messagesAttempted !== attemptedAccounted;
  const parseMismatch = messagesSkipped != null && messagesSkipped < 0;
  const hasIssues = summary.issues.length > 0;

  const messageRows: MessageRow[] = [
    { key: "parsed", label: "Parsed", value: summary.messagesParsed },
    { key: "skipped", label: "Skipped", value: messagesSkipped },
    { key: "attempted", label: "Attempted", value: summary.messagesAttempted },
    { key: "new", label: "New uploaded", value: summary.messagesInserted, indent: true },
    { key: "duplicate", label: "Duplicate", value: summary.messagesDeduped, indent: true },
    { key: "failed", label: "Failed", value: summary.messagesFailed, indent: true },
  ];

  return (
    <section className="mt-5">
      {embedStepTimings ? (
        <StepProgress
          steps={historySteps(summary)}
          completionText={completionTextFor(summary.status)}
        />
      ) : null}

      <div
        className={`${embedStepTimings ? "mt-4" : ""} grid min-w-0 grid-cols-1 gap-4 ${
          hasIssues ? "lg:grid-cols-2" : ""
        }`}
      >
        <div className="min-w-0 overflow-hidden rounded-lg border border-border">
          <table className="w-full table-fixed border-collapse text-[0.813rem]">
            <thead>
              <tr className="border-b border-border bg-elevated text-left text-muted">
                <th className="px-3 py-2 font-medium">Messages</th>
                <th className="w-28 px-3 py-2 text-right font-medium">Count</th>
              </tr>
            </thead>
            <tbody>
              {messageRows.map((row) => (
                <tr key={row.key} className="border-b border-border last:border-b-0">
                  <td className={`px-3 py-2 text-text ${row.indent ? "pl-8 text-muted" : ""}`}>
                    {row.label}
                  </td>
                  <td className="px-3 py-2 text-right tabular-nums text-text">
                    {formatCount(row.value)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        {hasIssues ? (
          <section className="min-w-0 overflow-hidden">
            <h2 className="m-0 text-base font-semibold">Import Errors</h2>
            <p className="mb-0 mt-1 text-[0.75rem] text-muted">
              Identical errors are grouped. Error messages show two lines. Click a row to expand the
              full message and the file list.
            </p>
            <VirtualizedImportIssuesTable issues={summary.issues} />
          </section>
        ) : null}
      </div>

      {attemptMismatch ? (
        <p className="mt-2 text-[0.813rem] text-danger">
          Message accounting mismatch: attempted does not equal new uploaded + duplicate + failed.
        </p>
      ) : null}
      {parseMismatch ? (
        <p className="mt-2 text-[0.813rem] text-danger">
          Message accounting mismatch: attempted exceeds parsed.
        </p>
      ) : null}
    </section>
  );
}

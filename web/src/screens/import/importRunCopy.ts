import type { ImportSummaryView } from "../../components/import/ImportSummaryPanel";
import { EXPORT_SOURCES } from "../../lib/exportSources";
import { IMESSAGE_METHODS, isImessageMethod } from "../../lib/imessageImport";
import { isWhatsappMethod, WHATSAPP_METHODS } from "../../lib/whatsappImport";
import { ATTACHMENT_OPTIONS } from "./ImportFormUi";
import type { ImportPhase } from "./importProgressState";
import type { ImportJobFormValues } from "./useImportJob";

/** Which of the run's two approvals (CONTEXT.md, "Approval"). */
export type ApprovalKind = "staging" | "media";

/** The person's name for the source, e.g. "iMessage · iPhone backup". */
export function sourceDisplayName(source: string): string {
  if (isImessageMethod(source)) {
    const method = IMESSAGE_METHODS.find((m) => m.id === source)?.label;
    return method ? `iMessage · ${method}` : "iMessage";
  }
  if (isWhatsappMethod(source)) {
    const method = WHATSAPP_METHODS.find((m) => m.id === source)?.label;
    return method ? `WhatsApp · ${method}` : "WhatsApp";
  }
  return EXPORT_SOURCES.find((s) => s.id === source)?.label ?? source;
}

/** The attachments line of "what you asked for", in the form's own words, settings included when they apply. */
export function attachmentsAsked(form: ImportJobFormValues): string {
  const label =
    ATTACHMENT_OPTIONS.find((o) => o.id === form.attachmentMedia)?.label ?? form.attachmentMedia;
  if (form.attachmentMedia !== "convert" && form.attachmentMedia !== "compress") return label;
  return `${label} · up to ${form.maxResolution}, ${form.maxFps} fps, files over ${form.minSizeMb} MB`;
}

/** The run view's heading: what the run is doing, or what it did. */
export function runHeading(
  phase: ImportPhase,
  form: ImportJobFormValues | null,
  summaryView: ImportSummaryView | null,
  completionText: string | undefined,
): string {
  if (phase !== "done") {
    return form ? `Importing from ${sourceDisplayName(form.source)}` : "Importing";
  }
  const status = summaryView?.status;
  const inserted = summaryView?.messagesInserted;
  if ((status === "completed" || status === "completed_with_issues") && inserted != null) {
    const imported = `Imported ${inserted.toLocaleString()} ${inserted === 1 ? "message" : "messages"}`;
    return status === "completed_with_issues" ? `${imported}, with errors` : imported;
  }
  return completionText ?? "Import finished";
}

/**
 * Name of the Contact Group the vault made for a finished run: the same
 * words `import_contact_group_name` (server) uses, so a link lands on it.
 */
export function importGroupName(
  source: string,
  finishedAt: string | null | undefined,
  startedAt: string,
): string {
  const date = (finishedAt ?? startedAt).slice(0, 10);
  return `${source} import ${date}`;
}

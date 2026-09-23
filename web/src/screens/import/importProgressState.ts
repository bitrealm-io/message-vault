import { formatAttachmentProgress } from "../../lib/attachmentProgressCopy";
import type { AttachmentMediaMode, ImportProgressEvent } from "../../lib/types";

export type ImportStep = {
  label: string;
  status: "pending" | "active" | "done" | "error";
  detail?: string;
  durationMs?: number | null;
};

/**
 * Where the Import screen is. The three stages of a run (CONTEXT.md,
 * "Stage") all show as `running`; the two reviews are phases of their
 * own, because the run is waiting for the person rather than working.
 */
export type ImportPhase =
  | "form"
  | "identity_stop"
  | "running"
  | "staging_review"
  | "media_review"
  | "done";

export type AttachmentProgressCounts = {
  done: number;
  total: number;
  bytesDone: number;
  bytesTotal: number;
};

/** The three stages of an Import Run, by the names the glossary gives them. */
export const STAGING_LABEL = "Staging";
export const MEDIA_LABEL = "Media";
export const UPLOAD_LABEL = "Upload";

/** Whether this attachment media mode runs a Media stage (convert/compress). */
export function hasMediaStage(mode: AttachmentMediaMode): boolean {
  return mode === "convert" || mode === "compress";
}

/**
 * The stage rows for this attachment media mode: three under convert and
 * compress, two under copy and skip. There is no Media stage in copy/skip,
 * so a greyed-out row would be promising work that will never run.
 */
export function stepsFor(mode: AttachmentMediaMode): ImportStep[] {
  const labels = [STAGING_LABEL, ...(hasMediaStage(mode) ? [MEDIA_LABEL] : []), UPLOAD_LABEL];
  return labels.map((label) => ({ label, status: "pending" }));
}

/**
 * Row index for every step this build knows, keyed by the step name — a
 * `Record` over `ImportProgressEvent["step"]` so a step added to that union
 * without a row here is a compile error, not a silent runtime fallback.
 *
 * Reading the backup, copying its attachments and writing the conversation
 * files are all Staging from the person's side, so the first four steps
 * narrate the one Staging row rather than rows of their own.
 */
const STEP_ROW_INDEX: Record<ImportProgressEvent["step"], (mode: AttachmentMediaMode) => number> = {
  setup: () => 0,
  parse: () => 0,
  attachments: () => 0,
  prepare: () => 0,
  media: (mode) => (hasMediaStage(mode) ? 1 : -1),
  upload: (mode) => (hasMediaStage(mode) ? 2 : 1),
};

/**
 * Index of the stage row that matches this event, in this mode's row list.
 * Returns -1 for a step with no row in this mode (`media` under copy/skip),
 * or for a step string this build does not recognise at all — the event
 * comes off the wire unvalidated, so a lookup miss must resolve to "no row",
 * never `undefined`. Callers must treat -1 as "no row to update", not index
 * with it.
 */
export function stepIndexFor(step: ImportProgressEvent["step"], mode: AttachmentMediaMode): number {
  return STEP_ROW_INDEX[step]?.(mode) ?? -1;
}

/**
 * Whether a progress event should mark its row done. Attachments stay active
 * until prepare, and a setup event never completes the Staging row: its
 * counts are "step 5 of 5", and the messages are still to be read once that
 * lands. Parse does not complete Staging either: the attachments and the
 * conversation files still follow it on the same row.
 */
export function isProgressStepComplete(
  step: ImportProgressEvent["step"],
  done: number,
  total: number,
): boolean {
  if (step === "attachments" || step === "setup" || step === "parse") return false;
  return total > 0 && done >= total;
}

/**
 * Detail line for a setup event: the step's label with its position, so a
 * long decrypt reads as "Deriving backup keys (1/5)" rather than a frozen
 * "Reading backup…".
 */
export function setupDetail(event: Pick<ImportProgressEvent, "done" | "total" | "status">): string {
  const label = event.status ?? "Preparing";
  return event.total > 0 ? `${label} (${event.done}/${event.total})` : label;
}

/** Done-line for the attachment copy, using the last live counts when present. */
export function attachmentDoneDetail(
  mode: AttachmentMediaMode,
  counts: AttachmentProgressCounts | null,
): string {
  return formatAttachmentProgress({
    mode,
    done: counts?.done ?? 0,
    total: counts?.total ?? 0,
    bytesDone: counts?.bytesDone ?? 0,
    bytesTotal: counts?.bytesTotal ?? 0,
  });
}

/**
 * When each Staging stage first and last reported, from `performance.now()`.
 * The stages overlap: messages are still being read while attachments copy,
 * and conversation files are written while attachments copy, so no stage's
 * event marks the end of another.
 */
export type StageTiming = {
  extractStartedAt: number | null;
  parseStartedAt: number | null;
  parseEndedAt: number | null;
  attachmentsStartedAt: number | null;
  attachmentsEndedAt: number | null;
  prepareStartedAt: number | null;
  prepareEndedAt: number | null;
};

export const EMPTY_TIMING: StageTiming = {
  extractStartedAt: null,
  parseStartedAt: null,
  parseEndedAt: null,
  attachmentsStartedAt: null,
  attachmentsEndedAt: null,
  prepareStartedAt: null,
  prepareEndedAt: null,
};

/**
 * Timing after one progress event at `now`: the event's stage starts at its
 * first event and ends at its latest. The progress sink always delivers a
 * stage's last count, so the latest event is when the stage finished.
 * Decrypting and caching (`setup`) are the start of reading the backup, so
 * they start the read timer without ending it.
 */
export function recordStageTime(
  timing: StageTiming,
  step: ImportProgressEvent["step"],
  now: number,
): StageTiming {
  switch (step) {
    case "setup":
      return { ...timing, parseStartedAt: timing.parseStartedAt ?? now };
    case "parse":
      return { ...timing, parseStartedAt: timing.parseStartedAt ?? now, parseEndedAt: now };
    case "attachments":
      return {
        ...timing,
        attachmentsStartedAt: timing.attachmentsStartedAt ?? now,
        attachmentsEndedAt: now,
      };
    case "prepare":
      return { ...timing, prepareStartedAt: timing.prepareStartedAt ?? now, prepareEndedAt: now };
    default:
      return timing;
  }
}

/**
 * Read, attachment, and prepare durations once extract has finished. A stage
 * that never reported took no time. Reading with no message count (only
 * setup events) runs until the extract finished.
 */
export function stageDurations(
  timing: StageTiming,
  extractFinishedAt: number,
): { parseMs: number; attachmentsMs: number; prepareMs: number } {
  const span = (start: number | null, end: number | null) =>
    start == null ? 0 : Math.max(0, (end ?? extractFinishedAt) - start);
  return {
    parseMs: span(timing.parseStartedAt ?? timing.extractStartedAt, timing.parseEndedAt),
    attachmentsMs: span(timing.attachmentsStartedAt, timing.attachmentsEndedAt),
    prepareMs: span(timing.prepareStartedAt, timing.prepareEndedAt),
  };
}

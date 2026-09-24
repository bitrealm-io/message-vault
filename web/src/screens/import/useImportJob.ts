import {
  completionTextFor,
  type ImportIssue,
  type ImportSummaryView,
} from "../../components/import/ImportSummaryPanel";
import { getBaseUrl } from "../../lib/api";
import { formatAttachmentProgress } from "../../lib/attachmentProgressCopy";
import { useAuth } from "../../lib/auth";
import { needsIdentityStop, parseSourceIdentities } from "../../lib/backupIdentity";
import { getDeviceId } from "../../lib/deviceId";
import { IMAZING_SOURCE_ID } from "../../lib/exportSources";
import { imessageExtractFields } from "../../lib/imessageExtractFields";
import { isImessageMethod } from "../../lib/imessageImport";
import type { ActiveImportSession } from "../../lib/importSession";
import {
  buildSourceFingerprint,
  discardImportSession,
  type ImportStage,
  setImportStage,
} from "../../lib/importSession";
import { mediaExtractFields, sbrExtractFields } from "../../lib/sbrExtractFields";
import { resolveImportStagingDir } from "../../lib/system-settings";
import {
  type AttachmentForecast,
  awaitTauriJob,
  invokeCancel,
  invokeDeleteStaging,
  invokeExtract,
  invokeImessageBackupIdentities,
  invokePathStat,
  invokePush,
  invokeSummarizeStaging,
  invokeTranscodeStaging,
  type OwnerHandleCount,
  onExtractEvents,
  type PushFinishedReport,
  probeFfmpegTools,
  type SizeVerdict,
  type StagingConfig,
  type StagingSummary,
  type TauriJobResult,
  type TranscodeFinishedReport,
} from "../../lib/tauri";
import { isTauri } from "../../lib/tauri-check";
import type { AttachmentMediaMode, ImportIssueEvent, ImportProgressEvent } from "../../lib/types";
import { useFetchAccountProfile } from "../../lib/useAccountProfile";
import { completeImport, createImport } from "../../lib/vaultApi";
import { importSessionCreateBody } from "../../lib/vaultSource";
import { whatsappExtractFields } from "../../lib/whatsappExtractFields";
import { isWhatsappMethod } from "../../lib/whatsappImport";
import { formSnapshot, isStringArray } from "./formSnapshot";
import { mediaJobVerb } from "./gateForecast";
import { importOutcome } from "./importOutcome";
import {
  type AttachmentProgressCounts,
  attachmentDoneDetail,
  EMPTY_TIMING,
  type ImportStep,
  isProgressStepComplete,
  MEDIA_LABEL,
  recordStageTime,
  STAGING_LABEL,
  type StageTiming,
  setupDetail,
  stageDurations,
  stepIndexFor,
  stepsFor,
  UPLOAD_LABEL,
} from "./importProgressState";
import {
  type ImportRunState,
  importRunStore,
  initialImportRunState,
  useImportRunState,
} from "./importRunStore";

export type { ImportPhase, ImportStep } from "./importProgressState";

export const PUSH_LOG_NAME = "vault-push.log";

/** Parse/attachments/prepare durations, fixed once extract finishes and read again at finish time. */
type ExtractDurations = {
  parseMs: number | null;
  attachmentsMs: number | null;
  prepareMs: number | null;
};

const EMPTY_DURATIONS: ExtractDurations = { parseMs: null, attachmentsMs: null, prepareMs: null };

/** Present-tense verb for the media step, following the mode so compress mode never says "Converting". */
function mediaVerb(mode: AttachmentMediaMode): string {
  return mode === "compress" ? "Compressing" : "Converting";
}

/** Sentence shown on the media step's row once the pass finishes. */
function mediaDoneDetail(mode: AttachmentMediaMode): string {
  return mode === "compress" ? "Compression complete" : "Conversion complete";
}

/**
 * True for the "canceled"/"cancelled" text a cancelled Tauri job's
 * `extract:error` carries. The media pass's own cancellation is spelled
 * "canceled" (one L, `transcode.rs`'s `check_cancel_now`); other layers of
 * the Rust side spell it "cancelled" (two L, `message-vault-io-core`'s
 * `check_cancel`) — matched case- and spelling-insensitively so this reads
 * either.
 */
function isCancellation(message: string): boolean {
  return /^cancell?ed$/i.test(message.trim());
}

/**
 * Extract stages originals regardless of the chosen media mode (ffmpeg is
 * only required once Gate 1 is approved, not up front) — convert and
 * compress run afterward, against the staged folder, via
 * `invokeTranscodeStaging`. Copy and skip pass through unchanged.
 */
function extractAttachmentMedia(mode: AttachmentMediaMode): AttachmentMediaMode {
  return mode === "convert" || mode === "compress" ? "copy" : mode;
}

/** The media fields `summarize_staging` and `transcode_staging` share, read from the submitted form. */
function stagingMediaFields(
  form: Pick<ImportJobFormValues, "attachmentMedia" | "maxResolution" | "maxFps" | "minSizeMb">,
): Pick<
  StagingConfig,
  "attachment_media" | "media_max_resolution" | "media_max_fps" | "media_min_size"
> {
  return mediaExtractFields({
    attachmentMedia: form.attachmentMedia,
    maxResolution: form.maxResolution,
    maxFps: form.maxFps,
    minSizeMb: form.minSizeMb,
  });
}

/** What a step is doing and what it counts, for every step but `media`
 * (which needs the mode — see `mediaVerb`), `setup` (which carries its own
 * label — see `setupDetail`) and `attachments` (which adds bytes — see
 * `formatAttachmentProgress`), keyed by step name so a step added to the
 * wire union without an entry here is a compile error rather than a silent
 * fallback.
 */
const STEP_LABEL: Record<
  Exclude<ImportProgressEvent["step"], "media" | "setup" | "attachments">,
  string
> = {
  parse: "Reading messages",
  prepare: "Preparing conversations",
  check: "Checking attachments",
  upload: "Uploading conversations",
};

/**
 * Label shown while a step is running. Falls back to a plain word for a
 * step string this build doesn't recognise — the event comes off the wire
 * unvalidated.
 */
function progressLabel(
  step: Exclude<ImportProgressEvent["step"], "setup" | "attachments">,
  mode: AttachmentMediaMode,
): string {
  if (step === "media") return `${mediaVerb(mode)} attachments`;
  return STEP_LABEL[step] ?? "Working";
}

/** Stage rows for this mode, with Staging optionally marked active. */
function initialSteps(
  status: ImportStep["status"] = "pending",
  attachmentMedia: AttachmentMediaMode = "copy",
): ImportStep[] {
  const steps = stepsFor(attachmentMedia);
  const first = steps[0];
  if (status === "active" && first) {
    steps[0] = { ...first, status, detail: "Reading backup…" };
  }
  return steps;
}

/**
 * Stage rows for a run resumed at a review or mid Media: Staging is
 * already done (nothing here re-extracts), Upload is always still pending
 * (nothing here has uploaded yet), and Media (when this mode has it) is done
 * only when `mediaDone` says the pass already finished in an earlier run. A
 * resume at `transcode` passes `mediaDone: false` and then calls
 * `runMediaPass`, which marks that same row active once it starts.
 */
function resumeSteps(attachmentMedia: AttachmentMediaMode, mediaDone: boolean): ImportStep[] {
  return stepsFor(attachmentMedia).map((step) => {
    if (step.label === STAGING_LABEL) return { ...step, status: "done", detail: "Already staged" };
    if (step.label === MEDIA_LABEL && mediaDone) {
      return { ...step, status: "done", detail: mediaDoneDetail(attachmentMedia) };
    }
    return step;
  });
}

export type ImportJobFormValues = {
  source: string;
  backupPath: string;
  backupPassword: string;
  attachmentMedia: AttachmentMediaMode;
  maxResolution: string;
  maxFps: string;
  minSizeMb: string;
  ownerPhones: string[];
  /** Owner email addresses; only SMS Backup+ reads them. */
  ownerEmails: string[];
  force: boolean;
  obfuscate: boolean;
  /** The IANA zone iMazing dates are read in: the account's, or the one picked
   * under Processing Options. Only the iMazing extract reads it, because its
   * dates carry no zone of their own. */
  timeZone: string;
  /** True for the Android SMS sources, whose extract carries owner phones. */
  isAndroidSms: boolean;
  attachmentRoot: string;
  appleContacts: string;
  whatsappKey: string;
  whatsappWa: string;
  whatsappMedia: string;
  whatsappDb: string;
  whatsappBusiness: boolean;
};

/** Pick up a session whose staging folder is already complete. */
/** A session whose copy was interrupted, and the folder it was writing into. */
export type ResumeWrite = {
  sessionId: number;
  stagingDir: string;
  /** The list recorded on the session at creation (`session.source_identities`,
   * parsed by the caller). A resumed write lands back on Gate 1 without
   * re-probing the backup, so this is the only way that gate's identity
   * section gets a list to show. */
  identities?: string[] | null;
};

export type ResumePush = {
  sessionId: number;
  stagingDir: string;
  /** The plan approved at the last gate this session passed, parsed from
   * its stored `summary` (`parseStoredStagingSummary`). Undefined when the
   * session recorded nothing usable — `runPush`/`finishImport` already
   * tolerate that absence, they just can't diff a resumed push's expected
   * omissions against it, which demotes an honest `completed` outcome to
   * `completed_with_issues` for exactly the interrupted-and-resumed case. */
  approved?: StagingSummary;
};

const SIZE_VERDICTS: readonly SizeVerdict[] = [
  "fits_as_is",
  "likely_fits",
  "may_grow",
  "probably_too_big",
  "cannot_process",
];

function isAttachmentForecast(value: unknown): value is AttachmentForecast {
  if (typeof value !== "object" || value === null) return false;
  const r = value as Record<string, unknown>;
  return (
    typeof r.path === "string" &&
    typeof r.name === "string" &&
    typeof r.sizeBytes === "number" &&
    typeof r.estimateBytes === "number" &&
    typeof r.verdict === "string" &&
    SIZE_VERDICTS.includes(r.verdict as SizeVerdict)
  );
}

function isOwnerHandleCount(value: unknown): value is OwnerHandleCount {
  if (typeof value !== "object" || value === null) return false;
  const r = value as Record<string, unknown>;
  return (
    typeof r.handle === "string" && typeof r.sent === "number" && typeof r.received === "number"
  );
}

/**
 * Parse a session's stored `summary` (Task 6) back into a `StagingSummary`
 * — the plan approved at the last gate the session passed.
 *
 * Read only as the *approved baseline* on resume, never shown directly:
 * decision 39 says the summary actually on screen is always recomputed
 * fresh from the folder. Like `restoreFormFromSnapshot`, this value came
 * from the database rather than from this session's own state, so its
 * shape is checked field by field rather than trusted; returns `undefined`
 * — not a throw — for anything that doesn't match. A resume with no usable
 * baseline still proceeds: `importOutcome` tolerates an absent one, it just
 * can't diff against one.
 */
export function parseStoredStagingSummary(raw: unknown): StagingSummary | undefined {
  if (typeof raw !== "object" || raw === null) return undefined;
  const r = raw as Record<string, unknown>;
  if (typeof r.conversations !== "number") return undefined;
  if (typeof r.messages !== "number") return undefined;
  if (!isStringArray(r.contactIdentifiers)) return undefined;
  if (!Array.isArray(r.ownerHandles) || !r.ownerHandles.every(isOwnerHandleCount)) {
    return undefined;
  }
  if (typeof r.attachments !== "number") return undefined;
  if (typeof r.attachmentBytes !== "number") return undefined;
  if (typeof r.verdictCounts !== "object" || r.verdictCounts === null) return undefined;
  const vc = r.verdictCounts as Record<string, unknown>;
  if (
    typeof vc.fitsAsIs !== "number" ||
    typeof vc.likelyFits !== "number" ||
    typeof vc.mayGrow !== "number" ||
    typeof vc.probablyTooBig !== "number" ||
    typeof vc.cannotProcess !== "number"
  ) {
    return undefined;
  }
  if (!Array.isArray(r.forecasts) || !r.forecasts.every(isAttachmentForecast)) return undefined;
  if (typeof r.assetMaxBytes !== "number") return undefined;

  return {
    conversations: r.conversations,
    messages: r.messages,
    contactIdentifiers: r.contactIdentifiers,
    ownerHandles: r.ownerHandles,
    attachments: r.attachments,
    attachmentBytes: r.attachmentBytes,
    verdictCounts: {
      fitsAsIs: vc.fitsAsIs,
      likelyFits: vc.likelyFits,
      mayGrow: vc.mayGrow,
      probablyTooBig: vc.probablyTooBig,
      cannotProcess: vc.cannotProcess,
    },
    forecasts: r.forecasts,
    assetMaxBytes: r.assetMaxBytes,
  };
}

/**
 * What one run remembers between its stages and never shows: timings,
 * issues, counts, the submitted form. One run at a time, so one of these;
 * it lives beside the store so the run survives the screen unmounting.
 */
type RunScratch = {
  /** The submitted form, parked while the identity stop is showing. */
  pendingIdentityForm: ImportJobFormValues | null;
  activeStep: ImportIssue["step"];
  issues: ImportIssue[];
  counts: { filesParsed?: number; messagesParsed?: number };
  timing: StageTiming;
  durations: ExtractDurations;
  importStartedAt: number;
  form: ImportJobFormValues | null;
  attachmentMode: AttachmentMediaMode;
  /**
   * What extract is doing to attachments right now: "copy" under
   * convert/compress too, since extract only stages originals; the Media
   * stage, not this, tells the convert/compress story. Kept apart from
   * `attachmentMode`, the mode the person chose, which drives the Media
   * row's wording and the row list's shape.
   */
  extractMediaMode: AttachmentMediaMode;
  lastAttachmentProgress: AttachmentProgressCounts | null;
  /**
   * The Staging row's latest line for each stage that reports on it. Reading
   * messages, copying attachments, and writing conversation files run at the
   * same time, so one shared line would flip between them on every event.
   */
  stagingLines: Partial<Record<StagingProgressStep, string>>;
  /** Guards approve and cancel against a double click doing the work twice. */
  reviewAction: boolean;
  /**
   * Guards startImport the same way: the identity probe awaits two network
   * calls before runImport ever sets `running`, so a double-click on Import
   * while that probe is in flight would otherwise start two runs.
   */
  startImport: boolean;
};

function freshScratch(): RunScratch {
  return {
    pendingIdentityForm: null,
    activeStep: "parse",
    issues: [],
    counts: {},
    timing: { ...EMPTY_TIMING },
    durations: { ...EMPTY_DURATIONS },
    importStartedAt: 0,
    form: null,
    attachmentMode: "copy",
    extractMediaMode: "copy",
    lastAttachmentProgress: null,
    stagingLines: {},
    reviewAction: false,
    startImport: false,
  };
}

let scratch: RunScratch = freshScratch();

const store = importRunStore;

/** Put the store and the scratch back to a fresh form. Tests call this between cases. */
export function resetImportRun(): void {
  scratch = freshScratch();
  store.reset(initialImportRunState(initialSteps()));
}

resetImportRun();

function updateSteps(update: (steps: ImportStep[]) => ImportStep[]): void {
  store.set((state) => ({ steps: update(state.steps) }));
}

/** Mark whichever row is active as failed. */
function failActiveStep(): void {
  updateSteps((steps) =>
    steps.map((step) => (step.status === "active" ? { ...step, status: "error" } : step)),
  );
}

function setRowByLabel(label: string, patch: Partial<ImportStep>): void {
  updateSteps((steps) =>
    steps.map((step) => (step.label === label ? { ...step, ...patch } : step)),
  );
}

/**
 * Back to the form. The run's record stays on the vault; only what the
 * screen holds goes. `resumeError` is kept on purpose (see the store).
 */
function returnToForm(): void {
  store.set({
    phase: "form",
    summaryView: null,
    stagingDir: null,
    importSessionId: null,
    stagingSummary: null,
    mediaSummary: null,
    mediaFailedCount: null,
    mediaToolsMissing: false,
    mediaPartiallyRan: false,
    computingSummary: false,
    form: null,
  });
}

/** Start a run's bookkeeping from nothing. */
function beginRun(form: ImportJobFormValues, firstStep: ImportIssue["step"]): void {
  scratch.importStartedAt = performance.now();
  scratch.activeStep = firstStep;
  scratch.issues = [];
  scratch.counts = {};
  scratch.timing = { ...EMPTY_TIMING };
  scratch.durations = { ...EMPTY_DURATIONS };
  scratch.lastAttachmentProgress = null;
  scratch.stagingLines = {};
  scratch.attachmentMode = form.attachmentMedia;
  scratch.extractMediaMode = extractAttachmentMedia(form.attachmentMedia);
  scratch.form = form;
}

function applyProgress(event: ImportProgressEvent): void {
  const now = performance.now();

  scratch.timing = recordStageTime(scratch.timing, event.step, now);
  if (event.step === "parse") {
    scratch.counts.messagesParsed =
      event.total > 0 && event.done >= event.total ? event.total : event.done;
  } else if (event.step === "attachments") {
    scratch.lastAttachmentProgress = {
      done: event.done,
      total: event.total,
      bytesDone: event.bytes_done ?? 0,
      bytesTotal: event.bytes_total ?? 0,
    };
  }

  const stepIndex = stepIndexFor(event.step, scratch.attachmentMode);
  // No row for this step in the current mode (or an unrecognised step off
  // the wire): leave activeStep pointing at whatever step has a row, so a
  // dropped event here never mislabels the next error.
  if (stepIndex < 0) return;
  // An issue raised during setup is a problem reading the backup, which is
  // what "parse" names in the Import Errors list.
  scratch.activeStep = event.step === "setup" ? "parse" : event.step;

  const detail = rowDetail(event);
  const done = isProgressStepComplete(event.step, event.done, event.total);

  updateSteps((current) =>
    current.map((step, index) => {
      if (index < stepIndex) {
        return { ...step, status: "done" };
      }
      if (index > stepIndex) return step;
      return {
        ...step,
        status: done ? "done" : "active",
        detail,
      };
    }),
  );
}

/** The progress steps that report on the Staging row. */
type StagingProgressStep = Extract<
  ImportProgressEvent["step"],
  "setup" | "parse" | "attachments" | "prepare" | "check"
>;

/** The Staging row's steps in the order their lines show: conversations,
 * then messages, then attachments. */
const STAGING_LINE_ORDER: readonly StagingProgressStep[] = [
  "setup",
  "prepare",
  "parse",
  "attachments",
  "check",
];

function isStagingProgressStep(step: ImportProgressEvent["step"]): step is StagingProgressStep {
  return (STAGING_LINE_ORDER as readonly string[]).includes(step);
}

/**
 * The row's detail for one progress event. On the Staging row this event
 * updates its own line and leaves the others in place; the setup line goes
 * once messages are being read, since setup is over by then.
 */
function rowDetail(event: ImportProgressEvent): string {
  const line = progressDetail(event);
  if (!isStagingProgressStep(event.step)) return line;
  scratch.stagingLines = { ...scratch.stagingLines, [event.step]: line };
  if (event.step !== "setup") delete scratch.stagingLines.setup;
  return STAGING_LINE_ORDER.flatMap((step) => scratch.stagingLines[step] ?? []).join("\n");
}

/** The detail line for one progress event. */
function progressDetail(event: ImportProgressEvent): string {
  if (event.step === "setup") return setupDetail(event);
  if (event.step === "attachments") {
    const last = scratch.lastAttachmentProgress;
    return formatAttachmentProgress({
      mode: scratch.extractMediaMode,
      done: event.done,
      total: event.total,
      bytesDone: event.bytes_done ?? last?.bytesDone ?? 0,
      bytesTotal: event.bytes_total ?? last?.bytesTotal ?? 0,
    });
  }
  const numbers = `${event.done.toLocaleString()}/${event.total.toLocaleString()}`;
  const counts = event.status ? `${numbers} (${event.status})` : numbers;
  return `${progressLabel(event.step, scratch.attachmentMode)}: ${counts}`;
}

function recordIssue(issue: ImportIssueEvent): void {
  scratch.issues = [...scratch.issues, issue];
}

function recordError(step: ImportIssue["step"], message: string): void {
  scratch.issues = [...scratch.issues, { kind: "error", step, item: "Import", reason: message }];
}

/** Run one desktop job to its end, feeding its progress and issues into the run. */
function runJob(invokeFn: () => Promise<void>): Promise<TauriJobResult> {
  return awaitTauriJob(invokeFn, undefined, applyProgress, recordIssue);
}

/**
 * `invokeSummarizeStaging`, with a listener on the same `extract:progress`
 * channel the extract and media passes use. `summarize_staging` (Rust)
 * emits progress on the `check` step while it walks a big folder, and
 * `applyProgress` already knows to draw that on the Staging row, so this
 * only has to make sure the event reaches it.
 */
async function summarizeStagingWithProgress(config: StagingConfig): Promise<StagingSummary> {
  const unlisten = await onExtractEvents({
    onLog: () => {},
    onProgress: applyProgress,
    onFinished: () => {},
    onError: () => {},
  });
  try {
    return await invokeSummarizeStaging(config);
  } finally {
    unlisten();
  }
}

/**
 * Move a live run to another stage, carrying the summary the person just
 * approved when there is one. `approvedPlan` is simply forwarded, undefined
 * and all: `setImportStage` posts `{ stage, summary: approvedPlan }`, and
 * `JSON.stringify` drops an `undefined`-valued property outright, so an
 * omitted plan and an explicit `undefined` reach the server identically —
 * no `summary` key at all, leaving whatever plan is already stored untouched.
 */
async function moveStage(
  sessionId: number,
  stage: ImportStage,
  approvedPlan?: StagingSummary,
): Promise<void> {
  await setImportStage(sessionId, stage, approvedPlan).catch(() => {});
}

/** True when ffmpeg is needed for this mode and cannot be found. */
async function mediaToolsMissingFor(mode: AttachmentMediaMode): Promise<boolean> {
  if (mediaJobVerb(mode) === null) return false;
  try {
    const probe = await probeFfmpegTools(null);
    return !probe.ok;
  } catch {
    return true;
  }
}

/** Stop at a review: the run waits, and the review takes the screen. */
function waitAtReview(phase: "staging_review" | "media_review"): void {
  store.set({ phase, running: false, computingSummary: false });
}

/**
 * Build the finished-import summary, record it, and (usually) post
 * `/complete`, the terminal step for every path except one: a failure
 * before either review, a failed Media stage, or an Upload that ran to
 * completion or failed all complete normally.
 *
 * `canceled` overrides `importOutcome`'s verdict outright: the person asked
 * for this, so it is never read as a failure.
 *
 * `skipComplete` is that one exception. A cancellation mid Media is routed
 * to the same recovery as a crash at that stage, and only an explicit
 * cancel from a review ends a waiting run: `/complete` is what ends one.
 * Posting it here would free the one-live-run slot and drop the run out of
 * `GET /v1/imports?status=running`, stranding the staged folder (and the
 * time already spent on it) with no run left to resume it through. The
 * caller sets this for a cancelled Media stage and for a cancelled Staging:
 * both resume from what is already on disk, so both are worth keeping. A
 * genuine failure at either stage still completes normally (a broken ffmpeg
 * or an unreadable backup must not lock the account out of importing).
 */
async function finishImport(args: {
  sessionId: number | null;
  threw: boolean;
  canceled?: boolean;
  pushReport: PushFinishedReport | null;
  uploadMs: number | null;
  skipComplete?: boolean;
  /**
   * The plan the person approved at their last review: the Media
   * Review's recomputed summary when there was a Media stage, the Staging
   * Review's otherwise. Only `runPush` has one to offer; every other call
   * into this function ends in `pushReport: null`, which fails the outcome
   * regardless of `approved`, so leaving it undefined there is a no-op.
   */
  approved?: StagingSummary;
}): Promise<void> {
  const { sessionId, threw, canceled, pushReport, uploadMs, skipComplete, approved } = args;
  const { parseMs, attachmentsMs, prepareMs } = scratch.durations;
  const durationMs = performance.now() - scratch.importStartedAt;
  const outcome: ImportSummaryView["status"] = canceled
    ? "canceled"
    : importOutcome({
        report: pushReport ?? undefined,
        threw,
        issues: scratch.issues,
        approved,
      });
  const finalSummary: ImportSummaryView = {
    status: outcome,
    ...scratch.counts,
    filesTotal: pushReport?.conversations_total ?? scratch.counts.filesParsed,
    filesSucceeded: pushReport?.conversations_ok,
    filesFailed: pushReport?.conversations_failed,
    filesSkipped: pushReport?.conversations_skipped,
    messagesAttempted: pushReport?.messages_attempted,
    messagesInserted: pushReport?.messages_inserted,
    messagesDeduped: pushReport?.messages_deduped,
    messagesFailed: pushReport?.messages_failed,
    attachmentsUploaded: pushReport?.assets_uploaded,
    parseMs,
    attachmentsMs,
    prepareMs,
    uploadMs,
    durationMs,
    issues: scratch.issues,
  };
  // Keyed by label: the Staging row folds reading, attachments and prepare
  // into one duration, and a mode with no Media stage has fewer rows.
  const stagingMs =
    parseMs != null || attachmentsMs != null || prepareMs != null
      ? (parseMs ?? 0) + (attachmentsMs ?? 0) + (prepareMs ?? 0)
      : null;
  const durationByLabel = new Map<string, number | null>([
    [STAGING_LABEL, stagingMs],
    [UPLOAD_LABEL, uploadMs],
  ]);
  updateSteps((current) =>
    current.map((step) => {
      const duration = durationByLabel.get(step.label);
      if (duration == null) return step;
      return { ...step, durationMs: duration };
    }),
  );
  const ok = outcome !== "failed" && outcome !== "canceled";
  if (sessionId && !skipComplete) {
    try {
      await completeImport(sessionId, {
        status: outcome,
        message_count: pushReport?.messages_inserted,
        attachment_count: pushReport?.assets_uploaded,
        bytes_uploaded: pushReport?.assets_bytes,
        parse_ms: parseMs,
        attachments_ms: attachmentsMs,
        prepare_ms: prepareMs,
        upload_ms: uploadMs,
        duration_ms: durationMs,
        summary: {
          files_total: finalSummary.filesTotal,
          files_succeeded: finalSummary.filesSucceeded,
          files_failed: finalSummary.filesFailed,
          files_skipped: finalSummary.filesSkipped,
          messages_parsed: finalSummary.messagesParsed,
          messages_attempted: finalSummary.messagesAttempted,
          messages_inserted: finalSummary.messagesInserted,
          messages_deduped: finalSummary.messagesDeduped,
          messages_failed: finalSummary.messagesFailed,
        },
        issues: finalSummary.issues,
      });
    } catch {
      // Completing the run on the vault is optional. The summary still shows local results.
    }
  }
  // Once the vault holds the import, the staging directory is a second,
  // unprotected copy of the person's messages in a temp folder, so it goes:
  // the push log, journal and report with it. The vault's own import record
  // (counts, timings, issues) is what stays. A failed or cancelled run keeps
  // its folder, since the staged files are what a retry would read.
  const stagingDir = ok ? await deleteStagingAfterSuccess() : store.get().stagingDir;
  // The vault writes this run's saved search and Contact Group when the run
  // completes, so a window closed mid-import still gets them.
  store.set({ summaryView: finalSummary, phase: "done", running: false, stagingDir });
}

/**
 * Delete the finished run's staging directory. Returns the directory the
 * screen should still show: `null` once the folder is gone, or the path when
 * deleting failed, so the person can still find what was left behind.
 */
async function deleteStagingAfterSuccess(): Promise<string | null> {
  const { stagingDir } = store.get();
  if (stagingDir == null) return null;
  try {
    await invokeDeleteStaging({ staging_dir: stagingDir });
    return null;
  } catch {
    // The import itself succeeded; the folder link stays so the person can
    // remove what is left by hand.
    return stagingDir;
  }
}

/**
 * Upload to the vault and record the outcome: the tail end shared by a
 * resumed run (jumps straight here), the Staging Review when there is no
 * Media stage, and the Media Review. Never throws: a push failure is
 * folded into the finished summary via `finishImport`, exactly like any
 * other terminal outcome.
 */
async function runPush(
  token: string | null,
  form: ImportJobFormValues,
  sessionId: number,
  outputDir: string,
  approvedPlan?: StagingSummary,
): Promise<void> {
  store.set({ running: true, phase: "running" });
  scratch.activeStep = "upload";
  setRowByLabel(UPLOAD_LABEL, { status: "active", detail: "Uploading to vault…" });
  await moveStage(sessionId, "pushing", approvedPlan);

  const uploadStartedAt = performance.now();
  let pushResult: TauriJobResult | null = null;
  let threw = false;
  try {
    const baseUrl = getBaseUrl();
    if (!token) throw new Error("Not authenticated");
    pushResult = await runJob(() =>
      invokePush({
        base_url: baseUrl,
        username: "",
        key: token,
        input_dir: outputDir,
        mode: "append",
        force: form.force,
        continue_on_error: true,
        skip_attachments: false,
        // Extract (or the Media stage) just wrote these files. Matching
        // size_bytes lets vault-push skip a second full-file hash.
        trust_export: true,
        import_id: sessionId,
      }),
    );
  } catch (e: unknown) {
    threw = true;
    recordError(scratch.activeStep, e instanceof Error ? e.message : String(e));
    failActiveStep();
  }
  const uploadMs = performance.now() - uploadStartedAt;
  if (!threw) {
    setRowByLabel(UPLOAD_LABEL, {
      status: "done",
      detail: "Upload complete",
      durationMs: uploadMs,
    });
  }

  await finishImport({
    sessionId,
    threw,
    pushReport: pushResult?.report ?? null,
    uploadMs,
    approved: approvedPlan,
  });
}

/**
 * Convert or compress the staged files after the Staging Review, then
 * recompute the summary against the folder as it now stands (the folder is
 * the truth, not the last estimate) and stop at the Media Review. A
 * failed stage ends the import the same way a failed Upload does, never a
 * silent fall-through to Upload.
 *
 * `approvedSummary` is undefined on a resume whose stored plan failed to
 * parse (`parseStoredStagingSummary`): `moveStage` tolerates that absence,
 * so the stage still runs rather than blocking
 * the resume over a plan that can no longer be read.
 */
async function runMediaPass(
  form: ImportJobFormValues,
  sessionId: number,
  outputDir: string,
  approvedSummary?: StagingSummary,
): Promise<void> {
  store.set({ running: true, phase: "running" });
  scratch.activeStep = "media";
  setRowByLabel(MEDIA_LABEL, { status: "active", detail: `${mediaVerb(form.attachmentMedia)}…` });

  // Carries the plan approved at the Staging Review even on this stage: a
  // crash mid-pass must not leave `summary_json` null with no baseline for
  // a later resume to diff against.
  await moveStage(sessionId, "transcode", approvedSummary);

  const mediaStartedAt = performance.now();
  let transcodeReport: TranscodeFinishedReport | undefined;
  let threw = false;
  let canceled = false;
  try {
    const result = await runJob(() =>
      invokeTranscodeStaging({
        staging_dir: outputDir,
        ...stagingMediaFields(form),
      }),
    );
    transcodeReport = result.transcode;
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e);
    if (isCancellation(msg)) {
      // The person asked for this: not an error, so no issue row for it.
      canceled = true;
    } else {
      threw = true;
      recordError(scratch.activeStep, msg);
    }
  }
  const mediaMs = performance.now() - mediaStartedAt;

  if (threw || canceled) {
    failActiveStep();
    // Neither path writes another stage: the run stays at `transcode`,
    // which is exactly where it got to. A cancellation also skips
    // `/complete` outright (see finishImport), so the run stays running and
    // resumable instead of completing and freeing the slot out from under a
    // staged folder nobody can reach any more. A failed stage still
    // completes normally: the account must not be locked out of importing
    // by a broken ffmpeg.
    await finishImport({
      sessionId,
      threw,
      canceled,
      pushReport: null,
      uploadMs: null,
      skipComplete: canceled,
    });
    return;
  }

  setRowByLabel(MEDIA_LABEL, {
    status: "done",
    detail: mediaDoneDetail(form.attachmentMedia),
    durationMs: mediaMs,
  });

  store.set({ computingSummary: true });
  try {
    const actual = await summarizeStagingWithProgress({
      staging_dir: outputDir,
      ...stagingMediaFields(form),
    });
    store.set({ mediaSummary: actual, mediaFailedCount: transcodeReport?.failed ?? null });
    await moveStage(sessionId, "awaiting_gate_2", approvedSummary);
    waitAtReview("media_review");
  } catch (e: unknown) {
    // The stage itself succeeded; only the recompute after it failed. Still
    // a failed import, not an unhandled rejection on a frozen review, and
    // still no later stage written, so the run stays at `transcode`.
    recordError("media", e instanceof Error ? e.message : String(e));
    store.set({ computingSummary: false });
    await finishImport({ sessionId, threw: true, pushReport: null, uploadMs: null });
  }
}

/** Fields extract needs for this form's source. */
function extractFieldsFor(form: ImportJobFormValues) {
  const attachmentMedia = extractAttachmentMedia(form.attachmentMedia);
  const media = {
    attachmentMedia,
    maxResolution: form.maxResolution,
    maxFps: form.maxFps,
    minSizeMb: form.minSizeMb,
  };
  if (isImessageMethod(form.source)) {
    return imessageExtractFields({
      source: form.source,
      backupPassword: form.backupPassword,
      ...media,
      obfuscate: form.obfuscate,
      attachmentRoot: form.attachmentRoot,
      appleContacts: form.appleContacts,
    });
  }
  if (isWhatsappMethod(form.source)) {
    return whatsappExtractFields({
      source: form.source,
      ...media,
      key: form.whatsappKey,
      wa: form.whatsappWa,
      media: form.whatsappMedia,
      db: form.whatsappDb,
      business: form.whatsappBusiness,
    });
  }
  if (form.isAndroidSms) {
    return sbrExtractFields({
      ...media,
      ownerPhones: form.ownerPhones,
      ownerEmails: form.ownerEmails,
      obfuscate: form.obfuscate,
    });
  }
  if (form.source === IMAZING_SOURCE_ID) {
    return { timezone: form.timeZone };
  }
  return {};
}

async function runImport(
  token: string | null,
  form: ImportJobFormValues,
  identities: string[] | null,
  resume?: ResumePush,
  resumeWrite?: ResumeWrite,
): Promise<void> {
  if (!isTauri()) return;
  beginRun(form, "parse");
  store.set({
    running: true,
    phase: "running",
    form,
    summaryView: null,
    stagingDir: null,
    importSessionId: null,
    stagingSummary: null,
    mediaSummary: null,
    mediaFailedCount: null,
    mediaToolsMissing: false,
    mediaPartiallyRan: false,
    resumeError: null,
    computingSummary: false,
  });

  let sessionId: number | null = null;

  try {
    if (!token) throw new Error("Not authenticated");

    if (resume) {
      // The staging folder is already complete, so there is nothing to
      // resolve, no new run to create (the account already has this one),
      // and no extract to run. resume_push is only ever offered after the
      // last review, so there IS a plan from it: it rides along as
      // `resume.approved` (parsed from the run's stored summary) when it
      // parses. Straight to Upload.
      const outputDir = resume.stagingDir;
      sessionId = resume.sessionId;
      store.set({
        stagingDir: outputDir,
        importSessionId: sessionId,
        steps: stepsFor(form.attachmentMedia).map((step) =>
          step.label === UPLOAD_LABEL
            ? { ...step, status: "active", detail: "Uploading to vault…" }
            : { ...step, status: "done", detail: "Already staged" },
        ),
      });
      await runPush(token, form, sessionId, outputDir, resume.approved);
      return;
    }

    // Only a run that extracts starts from the fresh list; the resume above
    // built its own, so setting this first would be overwritten.
    store.set({ steps: initialSteps("active", form.attachmentMedia) });

    let outputDir: string;
    if (resumeWrite) {
      // The run already exists and its Staging was interrupted. Reuse it and
      // its staging folder: the exporter reads the backup again and skips
      // the conversations already written.
      outputDir = resumeWrite.stagingDir;
      sessionId = resumeWrite.sessionId;
      store.set({ stagingDir: outputDir, importSessionId: sessionId });
      setRowByLabel(STAGING_LABEL, { detail: "Extracting…" });
      await moveStage(sessionId, "write");
    } else {
      outputDir = await resolveImportStagingDir(form.backupPath, form.source);
      store.set({ stagingDir: outputDir });

      const backupStat = await invokePathStat(form.backupPath).catch(() => null);
      const importSession = await createImport({
        ...importSessionCreateBody(form.source),
        stage: "parse",
        staging_dir: outputDir,
        device_id: getDeviceId(),
        form: formSnapshot(form),
        source_fingerprint: backupStat ? buildSourceFingerprint(form.backupPath, backupStat) : null,
        source_identities: identities,
      });
      sessionId = importSession.id;
      store.set({ importSessionId: sessionId });
      setRowByLabel(STAGING_LABEL, { detail: "Extracting…" });
      await moveStage(sessionId, "write");
    }

    scratch.timing.extractStartedAt = performance.now();
    const extractResult = await runJob(() =>
      invokeExtract({
        source: form.source,
        path: form.backupPath,
        output_dir: outputDir,
        ...(resumeWrite ? { resume: true } : {}),
        ...extractFieldsFor(form),
      }),
    );
    if (extractResult.extraction) {
      scratch.counts.filesParsed = extractResult.extraction.files_parsed;
      scratch.counts.messagesParsed = extractResult.extraction.messages_parsed;
    }

    const extractFinishedAt = performance.now();
    const { parseMs, attachmentsMs, prepareMs } = stageDurations(scratch.timing, extractFinishedAt);
    scratch.durations = { parseMs, attachmentsMs, prepareMs };
    // What extract did ("Copied", not "Converted", under convert/compress
    // too), as the Staging row's done line.
    const attachmentDoneLine = attachmentDoneDetail(
      extractAttachmentMedia(form.attachmentMedia),
      scratch.lastAttachmentProgress,
    );
    store.set({
      steps: stepsFor(form.attachmentMedia).map((step) =>
        step.label === STAGING_LABEL
          ? {
              ...step,
              status: "done" as const,
              detail: attachmentDoneLine,
              durationMs: parseMs + attachmentsMs + prepareMs,
            }
          : // Media and Upload: not run yet, the Staging Review comes first.
            step,
      ),
      computingSummary: true,
    });

    await moveStage(sessionId, "awaiting_gate_1");
    // The extract itself is done and staged: an error from here on is a
    // failed read of a folder that already holds the staged work, not a run
    // that failed. Routing it through the outer catch (below) would post
    // `/complete` and end the run, stranding that work with no way back to
    // it. This mirrors `resumeAtGate`'s landing exactly: return to the form
    // instead, surfacing the failure on `resumeError`. The stage already
    // written above (`awaiting_gate_1`) stays as it is: the next visit's
    // resume check finds the same run and offers this recompute again.
    try {
      const summary = await summarizeStagingWithProgress({
        staging_dir: outputDir,
        ...stagingMediaFields(form),
      });
      const toolsMissing = await mediaToolsMissingFor(form.attachmentMedia);
      store.set({ stagingSummary: summary, mediaToolsMissing: toolsMissing });
      waitAtReview("staging_review");
    } catch (e: unknown) {
      store.set({
        resumeError: e instanceof Error ? e.message : String(e),
        computingSummary: false,
        running: false,
      });
      returnToForm();
    }
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e);
    // A cancelled Staging is not a failure: the conversations already
    // written are real work, and Staging can pick up from them. Leaving the
    // run at `write` is what lets the next Import visit offer that. A
    // genuine failure still completes: a broken backup must not lock the
    // account out of importing.
    const canceled = isCancellation(msg);
    if (!canceled) recordError(scratch.activeStep, msg);
    failActiveStep();
    store.set({ computingSummary: false });
    await finishImport({
      sessionId,
      threw: !canceled,
      canceled,
      pushReport: null,
      uploadMs: null,
      skipComplete: canceled,
    });
  }
}

/**
 * Cancel the run from a review: close the run on the vault and delete
 * the staging folder. Both halves run regardless of the other's outcome: a
 * live run with no folder blocks the next import, and a folder with no run
 * is litter nothing will ever clean up.
 */
async function cancelRun(): Promise<void> {
  if (scratch.reviewAction) return;
  scratch.reviewAction = true;
  try {
    const { importSessionId: sessionId, stagingDir: outputDir } = store.get();
    await Promise.allSettled([
      sessionId != null ? discardImportSession(sessionId) : Promise.resolve(),
      outputDir != null ? invokeDeleteStaging({ staging_dir: outputDir }) : Promise.resolve(),
    ]);
  } finally {
    scratch.reviewAction = false;
  }
  returnToForm();
}

/** Stop the stage that is running. The run stays where it got to. */
async function cancel(): Promise<void> {
  await invokeCancel();
}

/**
 * Run an import through its stages and reviews, and keep the run's
 * state where the screen can read it (`importRunStore`).
 *
 * Every function here reads the run from the store at the moment it is
 * called, never from a render's closure, so a screen that unmounted while a
 * stage ran and mounted again finds the run where it left it.
 */
export function useImportJob() {
  const fetchAccountProfile = useFetchAccountProfile();
  const { token } = useAuth();
  const state: ImportRunState = useImportRunState();

  /**
   * Start an import. For a fresh iMessage start this first reads which
   * addresses the backup's device sent from and compares them to the
   * profile; when nothing matches, it parks the form and stops at
   * `identity_stop`, before any run exists, so Cancel has nothing to clean
   * up. The probe fails open: a source it cannot read will fail in the
   * extractor moments later with the proper error.
   */
  async function startImport(
    form: ImportJobFormValues,
    resume?: ResumePush,
    resumeWrite?: ResumeWrite,
  ): Promise<void> {
    if (!isTauri()) return;
    // A second call while one is already probing or running is a no-op.
    if (scratch.startImport) return;
    scratch.startImport = true;
    try {
      let identities: string[] | null = null;
      if (!resume && !resumeWrite && isImessageMethod(form.source)) {
        // The probe reads the backup (and, for an encrypted one, decrypts
        // it) before any run exists, which can take seconds: mark the run
        // busy for that stretch so the Import button reflects it.
        store.set({ running: true });
        try {
          identities = await invokeImessageBackupIdentities({
            path: form.backupPath,
            ios: form.source === "imessage-ios",
            backupPassword: form.backupPassword,
          }).catch(() => []);
          store.set({ sourceIdentities: identities });
          const profile = await fetchAccountProfile();
          if (needsIdentityStop(identities, profile)) {
            scratch.pendingIdentityForm = form;
            store.set({ phase: "identity_stop" });
            return;
          }
        } finally {
          store.set({ running: false });
        }
      } else {
        store.set({ sourceIdentities: resumeWrite ? (resumeWrite.identities ?? null) : null });
      }
      await runImport(token, form, identities, resume, resumeWrite);
    } finally {
      scratch.startImport = false;
    }
  }

  /** Continue past the identity stop with the parked form. */
  async function continueAfterIdentityStop(): Promise<void> {
    const form = scratch.pendingIdentityForm;
    if (!form) return;
    scratch.pendingIdentityForm = null;
    await runImport(token, form, store.get().sourceIdentities);
  }

  /** Leave the identity stop; nothing was created, so only the phase moves. */
  function cancelIdentityStop(): void {
    scratch.pendingIdentityForm = null;
    returnToForm();
  }

  /** Approve the waiting review: Media after the Staging Review when there is one, Upload otherwise. */
  async function approve(): Promise<void> {
    if (!isTauri()) return;
    if (scratch.reviewAction) return;
    const form = scratch.form;
    const {
      phase,
      importSessionId: sessionId,
      stagingDir: outputDir,
      stagingSummary,
      mediaSummary,
    } = store.get();
    // What the person is approving: the folder as Media left it at the
    // Media Review, as Staging left it at the Staging Review.
    const approvedSummary = phase === "media_review" ? mediaSummary : stagingSummary;
    if (!form || sessionId == null || outputDir == null || approvedSummary == null) return;

    scratch.reviewAction = true;
    try {
      if (phase === "staging_review" && mediaJobVerb(form.attachmentMedia) !== null) {
        await runMediaPass(form, sessionId, outputDir, approvedSummary);
      } else {
        await runPush(token, form, sessionId, outputDir, approvedSummary);
      }
    } finally {
      scratch.reviewAction = false;
    }
  }

  /**
   * Resume a run the vault reports waiting at a review (`awaiting_gate_1`
   * / `awaiting_gate_2`) or mid Media (`transcode`).
   *
   * `approve` can't do this itself: it depends on what the store holds
   * (`stagingSummary`, the form, `stagingDir`, `importSessionId`) that a
   * reload has none of, and it branches on the phase rather than the run's
   * own stored stage. This rebuilds that state from `session` instead, then
   * routes exactly the way the normal flow would have got here.
   *
   * `resumedForm` is the caller's already-validated `restoreFormFromSnapshot`
   * result: the caller needs that check anyway (to fall back to
   * `settings_unreadable`), so this trusts it rather than parsing
   * `session.form` a second time.
   *
   * The folder is the truth. Every landing recomputes the summary fresh
   * from the staging folder; the run's stored `summary` is read only as the
   * approved baseline for the Media Review's delta and the Media stage's
   * own bookkeeping, never as something restored and shown directly.
   *
   * A recompute failing here is a transient read of the staging folder, not
   * a run that failed: only an explicit cancel ends a waiting run, so this
   * must not complete it or write a stage. It returns to the form instead
   * (the resume check there re-runs and finds the same run, so the panel
   * reappears; that is the retry) and leaves the failure on `resumeError`.
   */
  async function resumeAtGate(
    session: ActiveImportSession,
    resumedForm: ImportJobFormValues,
  ): Promise<void> {
    if (!isTauri()) return;
    if (
      session.stage !== "awaiting_gate_1" &&
      session.stage !== "awaiting_gate_2" &&
      session.stage !== "transcode"
    ) {
      return;
    }
    if (!session.staging_dir) return; // resumeDecisionFor guarantees this; defensive only.

    const sessionId = session.id;
    const outputDir = session.staging_dir;
    const approved = parseStoredStagingSummary(session.summary);

    beginRun(resumedForm, session.stage === "transcode" ? "media" : "parse");
    store.set({
      resumeError: null,
      form: resumedForm,
      summaryView: null,
      stagingDir: outputDir,
      importSessionId: sessionId,
      stagingSummary: null,
      mediaSummary: null,
      mediaFailedCount: null,
      mediaToolsMissing: false,
      mediaPartiallyRan: false,
      sourceIdentities: parseSourceIdentities(session.source_identities),
    });

    /** Recompute the summary from the folder, then land on the given review. */
    async function landOn(
      review: "staging_review" | "media_review",
      partiallyRan: boolean,
    ): Promise<void> {
      store.set({
        steps: resumeSteps(resumedForm.attachmentMedia, review === "media_review"),
        computingSummary: true,
        phase: "running",
        running: true,
      });
      try {
        const actual = await summarizeStagingWithProgress({
          staging_dir: outputDir,
          ...stagingMediaFields(resumedForm),
        });
        if (review === "staging_review") {
          const missing = await mediaToolsMissingFor(resumedForm.attachmentMedia);
          store.set({
            stagingSummary: actual,
            mediaToolsMissing: missing,
            mediaPartiallyRan: partiallyRan,
          });
        } else {
          // The Staging row shows the plan approved before Media, read
          // back from the run; the Media rows show the folder as it is now.
          // Media's own report is gone on a resume, so its failed count is
          // unknown rather than zero.
          store.set({
            stagingSummary: approved ?? null,
            mediaSummary: actual,
            mediaFailedCount: null,
          });
        }
        waitAtReview(review);
      } catch (e: unknown) {
        store.set({
          resumeError: e instanceof Error ? e.message : String(e),
          computingSummary: false,
          running: false,
        });
        returnToForm();
      }
    }

    if (session.stage === "awaiting_gate_1") {
      await landOn("staging_review", false);
      return;
    }
    if (session.stage === "awaiting_gate_2") {
      await landOn("media_review", false);
      return;
    }

    // transcode: Media died mid-run. Re-running it is safe (the stage is
    // resumable), so long as the tools it needs are there: a resume with
    // ffmpeg missing falls back to the Staging Review's recomputed
    // summary instead of starting a job that can only fail, using the same
    // `mediaToolsMissing` gate the normal flow shows there.
    if (await mediaToolsMissingFor(resumedForm.attachmentMedia)) {
      await landOn("staging_review", true);
      return;
    }
    store.set({ steps: resumeSteps(resumedForm.attachmentMedia, false) });
    await runMediaPass(resumedForm, sessionId, outputDir, approved);
  }

  return {
    phase: state.phase,
    steps: state.steps,
    running: state.running,
    form: state.form,
    summaryView: state.summaryView,
    stagingDir: state.stagingDir,
    importSessionId: state.importSessionId,
    stagingSummary: state.stagingSummary,
    mediaSummary: state.mediaSummary,
    mediaFailedCount: state.mediaFailedCount,
    mediaToolsMissing: state.mediaToolsMissing,
    mediaPartiallyRan: state.mediaPartiallyRan,
    resumeError: state.resumeError,
    computingSummary: state.computingSummary,
    completionText:
      state.phase === "done" ? completionTextFor(state.summaryView?.status) : undefined,
    sourceIdentities: state.sourceIdentities,
    startImport,
    continueAfterIdentityStop,
    cancelIdentityStop,
    approve,
    cancelRun,
    resumeAtGate,
    cancel,
    returnToForm,
  };
}

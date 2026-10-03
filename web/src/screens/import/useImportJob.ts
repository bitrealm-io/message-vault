import {
  completionTextFor,
  type ImportIssue,
  type ImportSummaryView,
} from "../../components/import/ImportSummaryPanel";
import type { ImportIssueStage } from "../../components/import/importIssueStage";
import { getAccountId, getBaseUrl } from "../../lib/api";
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
import { importSessionCreateBody, showsAttachmentOptions } from "../../lib/importSource";
import { CANCELLED_MESSAGE, createRunCancel, type RunCancel } from "../../lib/runCancel";
import { registerRunningUpload } from "../../lib/runningUpload";
import { sbrExtractFields } from "../../lib/sbrExtractFields";
import { completeImport, createImport, getServerState } from "../../lib/serverApi";
import { resolveImportStagingDir } from "../../lib/system-settings";
import {
  type AttachmentForecast,
  awaitTauriJob,
  invokeDeleteStaging,
  invokeExtract,
  invokeImessageBackupIdentities,
  invokePathStat,
  invokePush,
  invokeReadImportRunRecord,
  invokeSaveImportRunRecord,
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
  issueFromEvent,
  MEDIA_LABEL,
  recordStageTime,
  STAGING_LABEL,
  type StageTiming,
  setupDetail,
  stageDurations,
  stageForStep,
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
import {
  EMPTY_RUN_RECORD,
  filesSkippedOverRun,
  parseRunRecord,
  RUN_ERROR_ITEM,
  type RunPart,
  type RunRecord,
  recordToCarry,
  wholeRun,
} from "./runRecord";

export type { ImportPhase, ImportStep } from "./importProgressState";

export const PUSH_LOG_NAME = "message-crate-push.log";

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
 * The form an Import Run is started with, its attachment mode as the form
 * showed it. iMazing and OpenExtract show no Attachments field, so the
 * field may still hold what was chosen for another source; their runs copy
 * attachments, and the run's stored form says so.
 */
function withShownAttachmentMode(form: ImportJobFormValues): ImportJobFormValues {
  return showsAttachmentOptions(form.source) ? form : { ...form, attachmentMedia: "copy" };
}

/**
 * The attachment size limit this Import Run works to, in bytes. A run always
 * has one by the time a stage needs it: a new run reads the server's before
 * Staging, and a resumed run reads its own back from the stored form.
 */
function assetLimitOf(form: Pick<ImportJobFormValues, "assetMaxBytes">): number {
  if (typeof form.assetMaxBytes !== "number") {
    throw new Error("This Import Run has no attachment size limit stored with it.");
  }
  return form.assetMaxBytes;
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
 * resume at `media` passes `mediaDone: false` and then calls
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
  /** The holder's WhatsApp number: required on Android, a fallback on iPhone. */
  whatsappOwnerPhone: string;
  /**
   * The server's attachment size limit, in bytes, as this Import Run works
   * to it. Not a field the person fills in: a new run reads it from
   * `GET /v1/server` before Staging, and it is stored with the run in the
   * form snapshot, so a resume uses the number the run was staged and
   * reviewed against even when the owner has changed the limit since.
   */
  assetMaxBytes?: number;
};

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

/** Pick up a session whose staging folder is already complete. */
export type ResumePush = {
  sessionId: number;
  stagingDir: string;
  /** The plan approved at the last gate this session passed, parsed from
   * its stored `summary` (`parseStoredStagingSummary`). Undefined when the
   * session recorded nothing usable — `runPush` and `importOutcome`
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
  if (!Array.isArray(r.forecasts) || !r.forecasts.every(isAttachmentForecast)) return undefined;
  if (typeof r.assetMaxBytes !== "number") return undefined;

  return {
    conversations: r.conversations,
    messages: r.messages,
    contactIdentifiers: r.contactIdentifiers,
    ownerHandles: r.ownerHandles,
    attachments: r.attachments,
    attachmentBytes: r.attachmentBytes,
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
  activeStage: ImportIssueStage;
  issues: ImportIssue[];
  counts: { filesParsed?: number; messagesParsed?: number };
  timing: StageTiming;
  durations: ExtractDurations;
  importStartedAt: number;
  form: ImportJobFormValues | null;
  attachmentMode: AttachmentMediaMode;
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
   * The Cancel of the stages running now. A new one starts with each run and
   * each approve, so a Cancel never carries into a stage the person started
   * after it.
   */
  runCancel: RunCancel;
  /**
   * Guards startImport the same way: the identity probe awaits two network
   * calls before runImport ever sets `running`, so a double-click on Import
   * while that probe is in flight would otherwise start two runs.
   */
  startImport: boolean;
  /**
   * What the run's earlier parts recorded, read from the staging folder when
   * the run resumes (`runRecord.ts`). Empty for a run that started here.
   */
  carried: RunRecord;
};

function freshScratch(): RunScratch {
  return {
    pendingIdentityForm: null,
    activeStage: "staging",
    issues: [],
    counts: {},
    timing: { ...EMPTY_TIMING },
    durations: { ...EMPTY_DURATIONS },
    importStartedAt: 0,
    form: null,
    attachmentMode: "copy",
    lastAttachmentProgress: null,
    stagingLines: {},
    reviewAction: false,
    runCancel: createRunCancel(),
    startImport: false,
    carried: EMPTY_RUN_RECORD,
  };
}

let scratch: RunScratch = freshScratch();

const store = importRunStore;

/** Put the store and the scratch back to a fresh form, started by no account. */
export function resetImportRun(): void {
  scratch = freshScratch();
  store.reset(initialImportRunState(initialSteps()));
}

resetImportRun();

/**
 * What the Import screen's actions are doing now, whichever account started
 * them: a stage, a probe, a Review being cancelled. Settles once they stop.
 * `takeRunFor` waits on it before another account's run replaces them.
 */
let work: Promise<void> | null = null;

/** Run `action` as the Import screen's work of the moment (`work`). */
function asWork(action: () => Promise<void>): Promise<void> {
  const tracked = action().finally(() => {
    if (work === tracked) work = null;
  });
  work = tracked;
  return tracked;
}

/**
 * Make the store `accountId`'s, before that account starts or resumes a run.
 *
 * A store already the account's is left as it is. A run another account
 * started is stopped and let settle first, so nothing it does afterwards
 * lands in the new run: its stage is cancelled, which leaves the run open on
 * the server for that account to resume from its staged folder. Then the
 * store goes back to a fresh form, named with `accountId` (#1085).
 */
async function takeRunFor(accountId: number | null): Promise<void> {
  while (store.get().accountId !== accountId) {
    const running = work;
    if (running == null) {
      resetImportRun();
      store.set({ accountId });
      return;
    }
    await scratch.runCancel.cancel();
    await running.catch(() => {});
  }
}

/**
 * True when the account logged in now is not the one that started the run:
 * that account logged out while a stage ran. The server calls the run makes
 * go with the session logged in now, so the run must not make another.
 */
function accountLeft(): boolean {
  return getAccountId() !== store.get().accountId;
}

/**
 * Stop the run where it got to, the way a Cancel does, once the account that
 * started it has left. The run stays open for that account to resume.
 */
function stopIfAccountLeft(): void {
  if (accountLeft()) throw new Error(CANCELLED_MESSAGE);
}

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
 * Back to the form. The run's record stays on the server; only what the
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
    reviewError: null,
    form: null,
  });
}

/** Start a run's bookkeeping from nothing. */
function beginRun(form: ImportJobFormValues, firstStage: ImportIssueStage): void {
  scratch.importStartedAt = performance.now();
  scratch.activeStage = firstStage;
  scratch.issues = [];
  scratch.counts = {};
  scratch.timing = { ...EMPTY_TIMING };
  scratch.durations = { ...EMPTY_DURATIONS };
  scratch.lastAttachmentProgress = null;
  scratch.stagingLines = {};
  scratch.attachmentMode = form.attachmentMedia;
  scratch.form = form;
  scratch.runCancel = createRunCancel();
  scratch.carried = EMPTY_RUN_RECORD;
}

/** This part of the run as it stands now. */
function currentPart(report: PushFinishedReport | null, uploadMs: number | null): RunPart {
  return {
    issues: scratch.issues,
    durationMs: performance.now() - scratch.importStartedAt,
    ...scratch.durations,
    uploadMs,
    filesParsed: scratch.counts.filesParsed,
    messagesParsed: scratch.counts.messagesParsed,
    report,
  };
}

/**
 * Pick up what the run's earlier parts recorded, from its staging folder. A
 * record that cannot be read leaves the run with only what this part
 * records: the resume itself goes ahead.
 */
async function loadCarriedRecord(stagingDir: string): Promise<void> {
  try {
    scratch.carried = parseRunRecord(await invokeReadImportRunRecord({ staging_dir: stagingDir }));
  } catch {
    scratch.carried = EMPTY_RUN_RECORD;
  }
}

/**
 * Write the run's record so far into its staging folder, for the part that
 * resumes it. Called wherever the run stops with the run still open: at a
 * Review, and when `finishImport` leaves the run open. A failed write loses
 * only this part's record; the run itself is unaffected.
 */
async function saveCarriedRecord(
  report: PushFinishedReport | null = null,
  uploadMs: number | null = null,
): Promise<void> {
  const { stagingDir } = store.get();
  if (stagingDir == null) return;
  try {
    await invokeSaveImportRunRecord({
      staging_dir: stagingDir,
      record: recordToCarry(scratch.carried, currentPart(report, uploadMs)),
    });
  } catch {
    // Nothing to show: the run goes on, and a resume starts from the
    // record the folder already held.
  }
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
  // the wire): leave activeStage pointing at the stage of whatever step has a row, so a
  // dropped event here never mislabels the next error.
  if (stepIndex < 0) return;
  // An issue raised during setup is a problem reading the backup, which is
  // Staging in the Import Errors list.
  scratch.activeStage = stageForStep(event.step);

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
      mode: scratch.attachmentMode,
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

function recordIssue(event: ImportIssueEvent): void {
  scratch.issues = [...scratch.issues, issueFromEvent(event)];
}

function recordError(stage: ImportIssueStage, message: string): void {
  scratch.issues = [
    ...scratch.issues,
    { kind: "error", stage, item: RUN_ERROR_ITEM, reason: message },
  ];
}

/**
 * Run one desktop job to its end, feeding its progress and issues into the
 * run. A job that a Cancel came before is never started, and fails with
 * `CANCELLED_MESSAGE` the way a job cancelled while it runs does.
 */
function runJob(invokeFn: () => Promise<void>): Promise<TauriJobResult> {
  const guarded = scratch.runCancel.guard(invokeFn);
  return awaitTauriJob(
    "Import Run",
    async () => {
      stopIfAccountLeft();
      await guarded();
    },
    undefined,
    applyProgress,
    recordIssue,
  );
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
 * A stage change the server did not record. The run stops where the server
 * last recorded it, so a later visit resumes it from there.
 */
class StageNotRecordedError extends Error {}

/**
 * Move a live run to another stage, carrying the summary the person just
 * approved when there is one. `approvedPlan` is simply forwarded, undefined
 * and all: `setImportStage` posts `{ stage, summary: approvedPlan }`, and
 * `JSON.stringify` drops an `undefined`-valued property outright, so an
 * omitted plan and an explicit `undefined` reach the server identically —
 * no `summary` key at all, leaving whatever plan is already stored untouched.
 *
 * Throws `StageNotRecordedError` when the write fails. A later visit resumes
 * the run from the stage the server holds, so the caller must not go on to
 * work the server does not know the run reached.
 */
async function moveStage(
  sessionId: number,
  stage: ImportStage,
  approvedPlan?: StagingSummary,
): Promise<void> {
  try {
    await setImportStage(sessionId, stage, approvedPlan);
  } catch (e: unknown) {
    const reason = e instanceof Error ? e.message : String(e);
    throw new StageNotRecordedError(`Message Crate didn't record the run's progress: ${reason}`);
  }
}

/**
 * Record that the run is waiting at a review. A failure leaves the review on
 * screen with the error on it (`reviewError`), and approving writes the stage
 * again before anything else (`approve`). Returns whether the server has it.
 */
async function moveStageAtReview(
  sessionId: number,
  stage: "staging_review" | "media_review",
  approvedPlan?: StagingSummary,
): Promise<boolean> {
  try {
    await moveStage(sessionId, stage, approvedPlan);
    store.set({ reviewError: null });
    return true;
  } catch (e: unknown) {
    store.set({ reviewError: e instanceof Error ? e.message : String(e) });
    return false;
  }
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
 * Build the run's summary, record it, and end the run or leave it open.
 *
 * A run ends when the server takes `/complete`, and its staging folder is
 * deleted then: the server holds the record, and nothing will read the
 * folder again (#1233). That covers a finished Upload, and a failed Staging
 * or Media stage, which is discarded at once because nothing complete exists
 * to upload; ending it also frees the account to start a new import.
 *
 * Every other way out leaves the run open on the server, at the stage it
 * reached, with its folder and the record of this part in it
 * (`saveCarriedRecord`), and the next visit to Import offers it again:
 *
 * - `paused`: an Upload that did not send every conversation, by Pause or
 *   by failure (`importOutcome`). It posts no `/complete`.
 * - `skipComplete`: a cancelled Staging or Media stage, and a stage change
 *   the server did not record. Both resume from what is on disk; posting
 *   `/complete` would free the run's slot and strand the folder.
 * - A `/complete` the server refuses: the server still holds the run as
 *   running. A finished Upload then shows as paused, and its resume finds
 *   every message sent and posts `/complete` again.
 *
 * The summary and the completion cover the whole run, the earlier parts'
 * record (`scratch.carried`) included.
 */
async function finishImport(args: {
  sessionId: number | null;
  status: "completed" | "completed_with_issues" | "failed" | "cancelled" | "paused";
  pushReport: PushFinishedReport | null;
  uploadMs: number | null;
  skipComplete?: boolean;
}): Promise<void> {
  const { sessionId, status, pushReport, uploadMs, skipComplete } = args;
  const carried = scratch.carried;
  const whole = wholeRun(carried, currentPart(pushReport, uploadMs));
  const finalSummary: ImportSummaryView = {
    status,
    messagesParsed: whole.messagesParsed,
    filesTotal: pushReport?.conversations_total ?? whole.filesParsed,
    filesSucceeded: whole.filesSucceeded,
    filesFailed: pushReport?.conversations_failed,
    filesSkipped: pushReport ? filesSkippedOverRun(carried, pushReport) : undefined,
    messagesAttempted: whole.messagesAttempted,
    messagesInserted: whole.messagesInserted,
    messagesDeduped: whole.messagesDeduped,
    messagesFailed: pushReport?.messages_failed,
    attachmentsUploaded: whole.attachmentsUploaded,
    parseMs: whole.parseMs,
    attachmentsMs: whole.attachmentsMs,
    prepareMs: whole.prepareMs,
    uploadMs: whole.uploadMs,
    durationMs: whole.durationMs ?? null,
    issues: whole.issues,
  };
  // Keyed by label: the Staging row folds reading, attachments and prepare
  // into one duration, and a mode with no Media stage has fewer rows.
  const { parseMs, attachmentsMs, prepareMs } = whole;
  const stagingMs =
    parseMs != null || attachmentsMs != null || prepareMs != null
      ? (parseMs ?? 0) + (attachmentsMs ?? 0) + (prepareMs ?? 0)
      : null;
  const durationByLabel = new Map<string, number | null>([
    [STAGING_LABEL, stagingMs],
    [UPLOAD_LABEL, whole.uploadMs ?? null],
  ]);
  updateSteps((current) =>
    current.map((step) => {
      const duration = durationByLabel.get(step.label);
      if (duration == null) return step;
      return { ...step, durationMs: duration };
    }),
  );
  const posts = sessionId != null && !skipComplete && status !== "paused";
  let completeRefused: string | null = null;
  if (posts) {
    try {
      // The server counts the messages and attachments the run holds: a
      // resumed Upload's report counts only what the resume sent.
      await completeImport(sessionId, {
        status,
        bytes_uploaded: whole.bytesUploaded,
        parse_ms: whole.parseMs,
        attachments_ms: whole.attachmentsMs,
        prepare_ms: whole.prepareMs,
        upload_ms: whole.uploadMs,
        duration_ms: whole.durationMs,
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
    } catch (e: unknown) {
      completeRefused = e instanceof Error ? e.message : String(e);
    }
  }
  if (completeRefused != null) {
    if (status === "completed" || status === "completed_with_issues") {
      finalSummary.status = "paused";
      setRowByLabel(UPLOAD_LABEL, { status: "error", detail: "Paused" });
    }
    // Shown here only, since the server never took the issues it would be
    // recorded with.
    finalSummary.issues = [
      ...finalSummary.issues,
      {
        kind: "error",
        stage: "upload",
        item: RUN_ERROR_ITEM,
        reason: `Message Crate didn't record the import as finished: ${completeRefused}`,
      },
    ];
  }
  // A run with no server record at all (its creation failed) is ended too:
  // nothing will ever offer its folder again.
  const runEnded = sessionId == null || (posts && completeRefused == null);
  let stagingDir = store.get().stagingDir;
  if (runEnded) {
    stagingDir = await deleteStagingFolder();
  } else {
    await saveCarriedRecord(pushReport, uploadMs);
  }
  // The server writes this run's saved search and Contact Group when the run
  // completes, so a window closed mid-import still gets them.
  store.set({ summaryView: finalSummary, phase: "done", running: false, stagingDir });
}

/**
 * Delete an ended run's staging directory: the staged messages, the push
 * log, journal and report, and the run record. Returns the directory the
 * screen should still show: `null` once the folder is gone, or the path
 * when deleting failed, so the person can still find what was left behind.
 */
async function deleteStagingFolder(): Promise<string | null> {
  const { stagingDir } = store.get();
  if (stagingDir == null) return null;
  try {
    await invokeDeleteStaging({ staging_dir: stagingDir });
    return null;
  } catch {
    // The run has ended either way; the folder link stays so the person can
    // remove what is left by hand.
    return stagingDir;
  }
}

/**
 * Upload to the server and record the outcome: the tail end shared by a
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
  // Logging out pauses this Upload before it ends the session the push
  // sends (`lib/runningUpload.ts`), and waits until the pause is recorded.
  const runCancel = scratch.runCancel;
  const upload = uploadAndFinish(token, form, sessionId, outputDir, approvedPlan);
  const pause = async () => {
    await runCancel.cancel();
    await upload;
  };
  const ended = registerRunningUpload(pause);
  try {
    await upload;
  } finally {
    ended();
  }
}

/** `runPush` without the registration that lets logging out pause it. */
async function uploadAndFinish(
  token: string | null,
  form: ImportJobFormValues,
  sessionId: number,
  outputDir: string,
  approvedPlan?: StagingSummary,
): Promise<void> {
  store.set({ running: true, phase: "running" });
  scratch.activeStage = "upload";
  setRowByLabel(UPLOAD_LABEL, { status: "active", detail: "Uploading to Message Crate…" });
  try {
    await moveStage(sessionId, "upload", approvedPlan);
  } catch (e: unknown) {
    // The server still has the run at its review, so the run stays there
    // and is not completed: a later visit offers that review again.
    recordError("upload", e instanceof Error ? e.message : String(e));
    failActiveStep();
    await finishImport({
      sessionId,
      status: "failed",
      pushReport: null,
      uploadMs: null,
      skipComplete: true,
    });
    return;
  }

  const uploadStartedAt = performance.now();
  let pushResult: TauriJobResult | null = null;
  let threw = false;
  // A Pause that came before the push started: the guard refused the job.
  let pausedBeforeStart = false;
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
        skip_attachments: false,
        // Extract (or the Media stage) just wrote these files. Matching
        // size_bytes lets message-crate-push skip a second full-file hash.
        trust_export: true,
        import_id: sessionId,
        asset_max_bytes: assetLimitOf(form),
      }),
    );
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e);
    if (msg === CANCELLED_MESSAGE) {
      // The person asked for this: not an error, so no issue row for it.
      pausedBeforeStart = true;
    } else {
      threw = true;
      recordError(scratch.activeStage, msg);
      failActiveStep();
    }
  }
  const uploadMs = performance.now() - uploadStartedAt;
  const report = pushResult?.report ?? null;
  // An Upload that did not send every conversation, paused or failed, is
  // paused: the run stays at `upload` with its folder, and resuming it
  // sends only what the push journal does not list.
  const status = pausedBeforeStart
    ? "paused"
    : importOutcome({
        report: report ?? undefined,
        threw,
        issues: wholeRun(scratch.carried, currentPart(report, uploadMs)).issues,
        approved: approvedPlan,
      });
  if (status === "paused") {
    setRowByLabel(UPLOAD_LABEL, { status: "error", detail: "Paused", durationMs: uploadMs });
  } else {
    setRowByLabel(UPLOAD_LABEL, {
      status: "done",
      detail: "Upload complete",
      durationMs: uploadMs,
    });
  }

  await finishImport({ sessionId, status, pushReport: report, uploadMs });
}

/**
 * Convert or compress the staged files after the Staging Review, then
 * recompute the summary against the folder as it now stands (the folder is
 * the truth, not the last estimate) and stop at the Media Review. A
 * failed stage ends the import as failed and deletes its folder, never a
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
  scratch.activeStage = "media";
  setRowByLabel(MEDIA_LABEL, { status: "active", detail: `${mediaVerb(form.attachmentMedia)}…` });

  // Carries the plan approved at the Staging Review even on this stage: a
  // crash mid-pass must not leave `summary_json` null with no baseline for
  // a later resume to diff against.
  try {
    await moveStage(sessionId, "media", approvedSummary);
  } catch (e: unknown) {
    // The server still has the run at the Staging Review, so the run stays
    // there and is not completed: a later visit offers that review again.
    recordError("media", e instanceof Error ? e.message : String(e));
    failActiveStep();
    await finishImport({
      sessionId,
      status: "failed",
      pushReport: null,
      uploadMs: null,
      skipComplete: true,
    });
    return;
  }

  const mediaStartedAt = performance.now();
  let transcodeReport: TranscodeFinishedReport | undefined;
  let threw = false;
  let cancelled = false;
  try {
    const result = await runJob(() => invokeTranscodeStaging({ staging_dir: outputDir }));
    transcodeReport = result.transcode;
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e);
    if (msg === CANCELLED_MESSAGE) {
      // The person asked for this: not an error, so no issue row for it.
      cancelled = true;
    } else {
      threw = true;
      recordError(scratch.activeStage, msg);
    }
  }
  const mediaMs = performance.now() - mediaStartedAt;

  if (threw || cancelled) {
    failActiveStep();
    // Neither path writes another stage: the run stays at `media`,
    // which is exactly where it got to. A cancellation also skips
    // `/complete` outright (see finishImport), so the run stays running and
    // resumable instead of completing and freeing the slot out from under a
    // staged folder nobody can reach any more. A failed stage is discarded:
    // it completes as failed and its folder goes, so a broken ffmpeg does
    // not lock the account out of importing.
    await finishImport({
      sessionId,
      status: cancelled ? "cancelled" : "failed",
      pushReport: null,
      uploadMs: null,
      skipComplete: cancelled,
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
    const actual = await summarizeStagingWithProgress({ staging_dir: outputDir });
    store.set({ mediaSummary: actual, mediaFailedCount: transcodeReport?.failed ?? null });
    await moveStageAtReview(sessionId, "media_review", approvedSummary);
    await saveCarriedRecord();
    waitAtReview("media_review");
  } catch (e: unknown) {
    // The stage itself succeeded; only the recompute after it failed. Still
    // a failed Media stage, not an unhandled rejection on a frozen review:
    // the run completes as failed and its folder goes.
    recordError("media", e instanceof Error ? e.message : String(e));
    store.set({ computingSummary: false });
    await finishImport({ sessionId, status: "failed", pushReport: null, uploadMs: null });
  }
}

/**
 * Fields extract needs for this form's source. The media fields go only to
 * a source whose form shows them, as the person chose them: extract checks
 * them before anything is staged and records them for the later stages.
 */
function extractFieldsFor(form: ImportJobFormValues) {
  const media = {
    attachmentMedia: form.attachmentMedia,
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
      backupPassword: form.backupPassword,
      wa: form.whatsappWa,
      media: form.whatsappMedia,
      db: form.whatsappDb,
      business: form.whatsappBusiness,
      ownerPhone: form.whatsappOwnerPhone,
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
  submitted: ImportJobFormValues,
  identities: string[] | null,
  resume?: ResumePush,
  resumeWrite?: ResumeWrite,
): Promise<void> {
  if (!isTauri()) return;
  let form = withShownAttachmentMode(submitted);
  beginRun(form, "staging");
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
    reviewError: null,
    computingSummary: false,
  });

  let sessionId: number | null = null;

  try {
    if (!token) throw new Error("Not authenticated");

    if (!resume && !resumeWrite) {
      // A new Import Run works to the server's attachment size limit as it is
      // now. It goes into the form the run is created with, so every later
      // stage, and a resume, measures against this same number.
      const server = await getServerState();
      form = { ...form, assetMaxBytes: server.asset_max_bytes };
      scratch.form = form;
      store.set({ form });
    }

    if (resume) {
      // The staging folder is already complete, so there is nothing to
      // resolve, no new run to create (the account already has this one),
      // and no extract to run. resume_push is only ever offered after the
      // last review, so there IS a plan from it: it rides along as
      // `resume.approved` (parsed from the run's stored summary) when it
      // parses. Straight to Upload.
      const outputDir = resume.stagingDir;
      sessionId = resume.sessionId;
      await loadCarriedRecord(outputDir);
      store.set({
        stagingDir: outputDir,
        importSessionId: sessionId,
        steps: stepsFor(form.attachmentMedia).map((step) =>
          step.label === UPLOAD_LABEL
            ? { ...step, status: "active", detail: "Uploading to Message Crate…" }
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
      await loadCarriedRecord(outputDir);
      store.set({ stagingDir: outputDir, importSessionId: sessionId });
      setRowByLabel(STAGING_LABEL, { detail: "Extracting…" });
      await moveStage(sessionId, "write");
    } else {
      outputDir = await resolveImportStagingDir(form.backupPath, form.source);
      store.set({ stagingDir: outputDir });

      const backupStat = await invokePathStat(form.backupPath).catch(() => null);
      // The run is created in the account of the session logged in now.
      stopIfAccountLeft();
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
        asset_max_bytes: assetLimitOf(form),
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
    // What extract did, as the Staging row's done line.
    const attachmentDoneLine = attachmentDoneDetail(
      form.attachmentMedia,
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

    await moveStageAtReview(sessionId, "staging_review");
    // Staging's issues and times are only in memory until now, and the run
    // may be resumed from this Review after the app closes.
    await saveCarriedRecord();
    // The extract itself is done and staged: an error from here on is a
    // failed read of a folder that already holds the staged work, not a run
    // that failed. Routing it through the outer catch (below) would post
    // `/complete` and end the run, stranding that work with no way back to
    // it. This mirrors `resumeAtReview`'s landing exactly: return to the form
    // instead, surfacing the failure on `resumeError`. The stage already
    // written above (`staging_review`) stays as it is: the next visit's
    // resume check finds the same run and offers this recompute again.
    try {
      const summary = await summarizeStagingWithProgress({ staging_dir: outputDir });
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
    // `write` stage the server did not record stops the run where the server
    // has it, so it is not completed either. A genuine failure still
    // completes: a broken backup must not lock the account out of importing.
    const cancelled = msg === CANCELLED_MESSAGE;
    const stageNotRecorded = e instanceof StageNotRecordedError;
    if (!cancelled) recordError(scratch.activeStage, msg);
    failActiveStep();
    store.set({ computingSummary: false });
    await finishImport({
      sessionId,
      status: cancelled ? "cancelled" : "failed",
      pushReport: null,
      uploadMs: null,
      skipComplete: cancelled || stageNotRecorded,
    });
  }
}

/**
 * Cancel the run from a review: close the run on the server and delete
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
  await scratch.runCancel.cancel();
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
  const auth = useAuth();
  const token = auth.token;
  const accountId = auth.accountId ?? null;
  // The logged-in account's run, or the fresh form when another account's
  // run is in the store (#1085).
  const state: ImportRunState = useImportRunState();

  /** True when the run in the store is this account's to act on. */
  function ownsRun(): boolean {
    return store.get().accountId === accountId;
  }

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
    await takeRunFor(accountId);
    // A second call while one is already probing or running is a no-op.
    if (scratch.startImport) return;
    scratch.startImport = true;
    const started = scratch;
    await asWork(async () => {
      try {
        await startRun(form, resume, resumeWrite);
      } finally {
        started.startImport = false;
      }
    });
  }

  /** `startImport`, once the store is this account's and no other start is under way. */
  async function startRun(
    form: ImportJobFormValues,
    resume?: ResumePush,
    resumeWrite?: ResumeWrite,
  ): Promise<void> {
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
        // The account logged out during the probe: the profile just read
        // is another account's, and no run exists yet to resume.
        if (accountLeft()) return;
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
  }

  /** Continue past the identity stop with the parked form. */
  async function continueAfterIdentityStop(): Promise<void> {
    if (!ownsRun()) return;
    const form = scratch.pendingIdentityForm;
    if (!form) return;
    scratch.pendingIdentityForm = null;
    await asWork(() => runImport(token, form, store.get().sourceIdentities));
  }

  /** Leave the identity stop; nothing was created, so only the phase moves. */
  function cancelIdentityStop(): void {
    if (!ownsRun()) return;
    scratch.pendingIdentityForm = null;
    returnToForm();
  }

  /** Approve the waiting review: Media after the Staging Review when there is one, Upload otherwise. */
  async function approve(): Promise<void> {
    if (!isTauri()) return;
    if (!ownsRun()) return;
    if (scratch.reviewAction) return;
    const form = scratch.form;
    const {
      phase,
      importSessionId: sessionId,
      stagingDir: outputDir,
      stagingSummary,
      mediaSummary,
      reviewError,
    } = store.get();
    // What the person is approving: the folder as Media left it at the
    // Media Review, as Staging left it at the Staging Review.
    const approvedSummary = phase === "media_review" ? mediaSummary : stagingSummary;
    if (!form || sessionId == null || outputDir == null || approvedSummary == null) return;

    scratch.reviewAction = true;
    scratch.runCancel = createRunCancel();
    const approving = scratch;
    await asWork(async () => {
      try {
        // The review's own stage did not reach the server, so it is written
        // first: a later visit must find the run at this review.
        if (reviewError != null) {
          const recorded =
            phase === "media_review"
              ? await moveStageAtReview(sessionId, "media_review", stagingSummary ?? undefined)
              : await moveStageAtReview(sessionId, "staging_review");
          if (!recorded) return;
        }
        if (phase === "staging_review" && mediaJobVerb(form.attachmentMedia) !== null) {
          await runMediaPass(form, sessionId, outputDir, approvedSummary);
        } else {
          await runPush(token, form, sessionId, outputDir, approvedSummary);
        }
      } finally {
        approving.reviewAction = false;
      }
    });
  }

  /**
   * Resume a run the server reports waiting at a review (`staging_review`
   * / `media_review`) or mid Media (`media`).
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
   * approved plan, for the Staging row on the Media Review and the Media
   * stage's own bookkeeping.
   *
   * A recompute failing here is a transient read of the staging folder, not
   * a run that failed: only an explicit cancel ends a waiting run, so this
   * must not complete it or write a stage. It returns to the form instead
   * (the resume check there re-runs and finds the same run, so the panel
   * reappears; that is the retry) and leaves the failure on `resumeError`.
   */
  async function resumeAtReview(
    session: ActiveImportSession,
    resumedForm: ImportJobFormValues,
  ): Promise<void> {
    if (!isTauri()) return;
    await takeRunFor(accountId);
    await asWork(() => resumeRunAtReview(session, resumedForm));
  }

  /** `resumeAtReview`, once the store is this account's. */
  async function resumeRunAtReview(
    session: ActiveImportSession,
    resumedForm: ImportJobFormValues,
  ): Promise<void> {
    if (
      session.stage !== "staging_review" &&
      session.stage !== "media_review" &&
      session.stage !== "media"
    ) {
      return;
    }
    if (!session.staging_dir) return; // resumeDecisionFor guarantees this; defensive only.

    const sessionId = session.id;
    const outputDir = session.staging_dir;
    const approved = parseStoredStagingSummary(session.summary);

    beginRun(resumedForm, session.stage === "media" ? "media" : "staging");
    await loadCarriedRecord(outputDir);
    store.set({
      resumeError: null,
      reviewError: null,
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
        const actual = await summarizeStagingWithProgress({ staging_dir: outputDir });
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

    if (session.stage === "staging_review") {
      await landOn("staging_review", false);
      return;
    }
    if (session.stage === "media_review") {
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
    reviewError: state.reviewError,
    computingSummary: state.computingSummary,
    completionText:
      state.phase === "done" ? completionTextFor(state.summaryView?.status) : undefined,
    sourceIdentities: state.sourceIdentities,
    startImport,
    continueAfterIdentityStop,
    cancelIdentityStop,
    approve,
    // Another account's run is that account's to cancel, stop or leave.
    cancelRun: async () => {
      if (ownsRun()) await asWork(cancelRun);
    },
    resumeAtReview,
    cancel: async () => {
      if (ownsRun()) await cancel();
    },
    returnToForm: () => {
      if (ownsRun()) returnToForm();
    },
  };
}

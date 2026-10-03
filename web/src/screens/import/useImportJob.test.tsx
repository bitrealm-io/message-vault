/** @vitest-environment jsdom */

// Pins the wiring the 2026-08-27 incident broke: useImportJob must pass
// pushResult.report into importOutcome, use that outcome as
// finalSummary.status, and send it as `status` in the /complete POST body.
// The verdict logic itself is covered exhaustively by importOutcome.test.ts;
// nothing there would have caught a revert of the three lines that connect
// that logic to the hook, because those tests call importOutcome directly.
//
// It also pins the two-gate flow added afterward: startImport now stops at
// Gate 1 instead of pushing straight through, approve runs the media
// pass (when there is one) and stops at Gate 2, and cancelRun closes the
// session and deletes the staging folder. Every push assertion below goes
// through approve first, because there is no other way to reach it.

import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ActiveImportSession } from "../../lib/importSession";
import type {
  FfmpegToolsProbe,
  PushFinishedReport,
  StagingSummary,
  TauriJobResult,
} from "../../lib/tauri";
import type { AttachmentMediaMode, ImportIssueEvent, ImportProgressEvent } from "../../lib/types";
import { restoreFormFromSnapshot, snapshotSecret } from "./formSnapshot";
import { importRunStore } from "./importRunStore";

const createImportMock = vi.fn();
const getServerStateMock = vi.fn();
const completeImportMock = vi.fn();
const runMock = vi.fn<(fn: () => Promise<unknown>) => Promise<TauriJobResult>>();
const cancelMock = vi.fn();
const resolveImportStagingDirMock = vi.fn();
const invokePathStatMock = vi.fn();
const invokePushMock = vi.fn();
const invokeExtractMock = vi.fn();
const invokeSummarizeStagingMock = vi.fn();
const invokeTranscodeStagingMock = vi.fn();
const invokeDeleteStagingMock = vi.fn();
const probeFfmpegToolsMock = vi.fn<(dir: string | null) => Promise<FfmpegToolsProbe>>();
const setImportStageMock = vi.fn();
const discardImportSessionMock = vi.fn();
const invokeImessageBackupIdentitiesMock = vi.fn();
const loadAccountProfileMock = vi.fn();
const readRunRecordMock = vi.fn();
const saveRunRecordMock = vi.fn();

/**
 * `onExtractEvents` stands in for the real Tauri event listener. Its default
 * implementation just captures the callbacks it was given (so a test can
 * fire `onProgress` manually to simulate an event arriving mid-call) and
 * resolves to a no-op unlisten function — every summarize call site now
 * subscribes and unsubscribes around its `invokeSummarizeStaging`, so this
 * has to resolve for those call sites to complete at all.
 */
let lastExtractEventCallbacks: { onProgress?: (event: ImportProgressEvent) => void } | null = null;
const onExtractEventsMock = vi.fn(
  async (callbacks: { onProgress?: (event: ImportProgressEvent) => void }) => {
    lastExtractEventCallbacks = callbacks;
    return () => {};
  },
);

vi.mock("../../lib/tauri", () => ({
  // The job's name comes first; the canned results below take what follows it.
  awaitTauriJob: (_job: string, ...args: Parameters<typeof runMock>) => runMock(...args),
  invokeCancel: (...args: unknown[]) => cancelMock(...args),
  invokeExtract: (...args: unknown[]) => invokeExtractMock(...args),
  invokePush: (...args: unknown[]) => invokePushMock(...args),
  invokePathStat: (...args: unknown[]) => invokePathStatMock(...args),
  invokeSummarizeStaging: (...args: unknown[]) => invokeSummarizeStagingMock(...args),
  invokeTranscodeStaging: (...args: unknown[]) => invokeTranscodeStagingMock(...args),
  invokeDeleteStaging: (...args: unknown[]) => invokeDeleteStagingMock(...args),
  invokeReadImportRunRecord: (...args: unknown[]) => readRunRecordMock(...args),
  invokeSaveImportRunRecord: (...args: unknown[]) => saveRunRecordMock(...args),
  probeFfmpegTools: (...args: [string | null]) => probeFfmpegToolsMock(...args),
  invokeImessageBackupIdentities: (...args: unknown[]) =>
    invokeImessageBackupIdentitiesMock(...args),
  onExtractEvents: (...args: [{ onProgress?: (event: ImportProgressEvent) => void }]) =>
    onExtractEventsMock(...args),
}));

vi.mock("../../lib/useAccountProfile", () => ({
  useFetchAccountProfile:
    () =>
    (...args: unknown[]) =>
      loadAccountProfileMock(...args),
}));

vi.mock("../../lib/api", () => ({
  getBaseUrl: () => "http://127.0.0.1:8080",
  getAccountId: () => auth.accountId,
}));

// The three server calls this hook makes. Everything else in serverApi stays real,
// since other modules in this graph import from it.
vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  createImport: (...args: unknown[]) => createImportMock(...args),
  getServerState: (...args: unknown[]) => getServerStateMock(...args),
  completeImport: (...args: unknown[]) => completeImportMock(...args),
}));

/** The logged-in account. A test of two accounts on one desktop app changes it. */
let auth: { token: string | null; accountId: number | null } = {
  token: "test-token",
  accountId: 1,
};
vi.mock("../../lib/auth", () => ({
  useAuth: () => auth,
}));

vi.mock("../../lib/system-settings", () => ({
  resolveImportStagingDir: (...args: unknown[]) => resolveImportStagingDirMock(...args),
}));

vi.mock("../../lib/tauri-check", () => ({
  isTauri: () => true,
}));

vi.mock("../../lib/importSession", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/importSession")>();
  return {
    ...actual,
    setImportStage: (...args: unknown[]) => setImportStageMock(...args),
    discardImportSession: (...args: unknown[]) => discardImportSessionMock(...args),
  };
});

// Imported after the mocks above so useImportJob picks up the mocked modules.
const { useImportJob, parseStoredStagingSummary, resetImportRun } = await import("./useImportJob");

/**
 * `runMock` stands in for `awaitTauriJob`, which always calls the
 * invoke function it is given before resolving. Tests that assert on
 * `invokeExtract`/`invokeTranscodeStaging`/`invokePush` args need that same
 * behaviour, so every canned result below goes through this instead of
 * `mockResolvedValueOnce` (which would never call the function at all).
 */
function runResult(result: TauriJobResult) {
  return async (fn: () => Promise<unknown>) => {
    await fn();
    return result;
  };
}

/**
 * Like `runResult`, but also fires `onIssue` — the real `awaitTauriJob`
 * does this from the job's own event stream, which the mock above otherwise
 * never exercises. Needed to simulate a push that reports a skip.
 */
function runResultWithIssue(result: TauriJobResult, ...issues: ImportIssueEvent[]) {
  return async (
    fn: () => Promise<unknown>,
    _onLog?: (line: string) => void,
    _onProgress?: (event: ImportProgressEvent) => void,
    onIssue?: (event: ImportIssueEvent) => void,
  ) => {
    await fn();
    for (const issue of issues) onIssue?.(issue);
    return result;
  };
}

function failedReport(): PushFinishedReport {
  return {
    ok: false,
    cancelled: false,
    messages: 8_000,
    messages_attempted: 8_000,
    messages_inserted: 0,
    messages_deduped: 0,
    messages_failed: 8_000,
    assets_uploaded: 0,
    assets_bytes: 0,
    conversations_ok: 0,
    conversations_total: 681,
    conversations_failed: 681,
    conversations_skipped: 0,
    conversations_cancelled: 0,
    results: [],
  };
}

function okReport(overrides: Partial<PushFinishedReport> = {}): PushFinishedReport {
  return {
    ok: true,
    cancelled: false,
    messages: 10,
    messages_attempted: 10,
    messages_inserted: 10,
    messages_deduped: 0,
    messages_failed: 0,
    assets_uploaded: 0,
    assets_bytes: 0,
    conversations_ok: 1,
    conversations_total: 1,
    conversations_failed: 0,
    conversations_skipped: 0,
    conversations_cancelled: 0,
    results: [],
    ...overrides,
  };
}

function stagingSummary(overrides: Partial<StagingSummary> = {}): StagingSummary {
  return {
    conversations: 1,
    messages: 10,
    contactIdentifiers: [],
    ownerHandles: [],
    attachments: 0,
    attachmentBytes: 0,
    forecasts: [],
    assetMaxBytes: 50 * 1024 * 1024,
    ...overrides,
  };
}

const MIB = 1024 * 1024;

function okProbe(): FfmpegToolsProbe {
  return {
    ok: true,
    ffmpeg_path: "/usr/bin/ffmpeg",
    ffprobe_path: "/usr/bin/ffprobe",
    error: null,
  };
}

const EXTRACT_RESULT: TauriJobResult = {
  summary: "Extracted 8000 messages.",
  extraction: { files_parsed: 681, messages_parsed: 8_000 },
};

const baseForm = {
  source: "imessage-ios",
  backupPath: "/backups/iphone.tar",
  backupPassword: "",
  attachmentMedia: "copy" as const,
  maxResolution: "",
  maxFps: "",
  minSizeMb: "",
  ownerPhones: [],
  ownerEmails: [],
  obfuscate: false,
  isAndroidSms: false,
  attachmentRoot: "",
  appleContacts: "",
  whatsappKey: "",
  whatsappWa: "",
  whatsappMedia: "",
  whatsappDb: "",
  whatsappBusiness: false,
  whatsappOwnerPhone: "",
  timeZone: "America/New_York",
  // What a resumed run reads back from its stored form. A new run reads the
  // server's limit instead and replaces this.
  assetMaxBytes: 512 * MIB,
};

function form(overrides: { attachmentMedia?: AttachmentMediaMode } = {}) {
  return { ...baseForm, ...overrides };
}

describe("useImportJob wiring", () => {
  beforeEach(() => {
    resetImportRun();
    runMock.mockReset();
    // Every test's first run() call is extract; a test that also calls
    // approve() chains a second implementation for the media pass or
    // the push on top of this one.
    runMock.mockImplementationOnce(runResult(EXTRACT_RESULT));
    cancelMock.mockReset();
    getServerStateMock.mockReset();
    getServerStateMock.mockResolvedValue({ asset_max_bytes: 512 * MIB });
    createImportMock.mockReset();
    createImportMock.mockResolvedValue({ id: 1 });
    completeImportMock.mockReset();
    completeImportMock.mockResolvedValue({});
    resolveImportStagingDirMock.mockReset();
    resolveImportStagingDirMock.mockResolvedValue("/home/sam/message-crate/staging-iphone");
    invokePathStatMock.mockReset();
    invokePathStatMock.mockResolvedValue(null);
    invokeExtractMock.mockReset();
    invokePushMock.mockReset();
    readRunRecordMock.mockReset();
    saveRunRecordMock.mockReset();
    invokeSummarizeStagingMock.mockReset();
    invokeSummarizeStagingMock.mockResolvedValue(stagingSummary());
    invokeTranscodeStagingMock.mockReset();
    invokeDeleteStagingMock.mockReset();
    invokeDeleteStagingMock.mockResolvedValue(undefined);
    probeFfmpegToolsMock.mockReset();
    probeFfmpegToolsMock.mockResolvedValue(okProbe());
    setImportStageMock.mockReset();
    setImportStageMock.mockResolvedValue(undefined);
    discardImportSessionMock.mockReset();
    discardImportSessionMock.mockResolvedValue(undefined);
    invokeImessageBackupIdentitiesMock.mockReset();
    // No identities read by default, so the identity check never stops an
    // existing test that doesn't set up its own probe/profile response
    // (needsIdentityStop is a no-op on an empty list, fail-open by design).
    invokeImessageBackupIdentitiesMock.mockResolvedValue([]);
    loadAccountProfileMock.mockReset();
    loadAccountProfileMock.mockResolvedValue({ phones: [], emails: [] });
    createImportMock.mockReset();
    createImportMock.mockResolvedValue({ id: 1 });
    completeImportMock.mockReset();
    completeImportMock.mockResolvedValue({});
  });

  it("reads the server's attachment size limit before Staging and works to it through Upload", async () => {
    getServerStateMock.mockResolvedValue({ asset_max_bytes: 100 * MIB });
    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));

    // Stored with the Import Run when it is created, before anything is staged.
    expect(createImportMock).toHaveBeenCalledWith(
      expect.objectContaining({ form: expect.objectContaining({ assetMaxBytes: 100 * MIB }) }),
    );
    expect(getServerStateMock.mock.invocationCallOrder[0]).toBeLessThan(
      invokeExtractMock.mock.invocationCallOrder[0] as number,
    );
    // Staging writes it into the staged folder, where the Staging Review's
    // forecast and the Media stage read it.
    expect(invokeExtractMock).toHaveBeenCalledWith(
      expect.objectContaining({ asset_max_bytes: 100 * MIB }),
    );

    await act(() => result.current.approve());
    expect(invokePushMock).toHaveBeenCalledWith(
      expect.objectContaining({ asset_max_bytes: 100 * MIB }),
    );
  });

  it("gives Staging the media settings once and the later stages only the folder", async () => {
    // Compress with Max FPS cleared: extract is given the real mode and the
    // fields, so it refuses them before anything is staged.
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "compress" })));
    expect(invokeExtractMock).toHaveBeenCalledWith(
      expect.objectContaining({ attachment_media: "compress", media_max_fps: "" }),
    );
    await act(() => result.current.approve());

    // The summaries and the Media stage read the settings from the folder.
    const folder = { staging_dir: "/home/sam/message-crate/staging-iphone" };
    expect(invokeSummarizeStagingMock.mock.calls.map((call) => call[0])).toEqual([folder, folder]);
    expect(invokeTranscodeStagingMock).toHaveBeenCalledWith(folder);
  });

  it("an iMazing run, whose form shows no attachment option, has no Media stage", async () => {
    // Compress was chosen on the Apple Messages form; the source then
    // changed to iMazing and the field kept its value.
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );
    const { result } = renderHook(() => useImportJob());
    await act(() =>
      result.current.startImport({ ...form({ attachmentMedia: "compress" }), source: "imazing" }),
    );
    // extract gets no attachment mode for iMazing, so it stages originals.
    expect(JSON.stringify(invokeExtractMock.mock.calls[0])).not.toMatch(/attachment_?[mM]edia/);
    await act(() => result.current.approve());

    expect(invokeTranscodeStagingMock).not.toHaveBeenCalled();
  });

  it("records Copy for an iMazing or OpenExtract run whatever another source's form chose", async () => {
    // Skip was chosen on the Apple Messages form; the source then changed.
    for (const source of ["imazing", "openextract"]) {
      resetImportRun();
      createImportMock.mockClear();
      runMock.mockImplementationOnce(runResult(EXTRACT_RESULT));
      const { result } = renderHook(() => useImportJob());
      await act(() => result.current.startImport({ ...form({ attachmentMedia: "skip" }), source }));

      expect(createImportMock).toHaveBeenCalledWith(
        expect.objectContaining({ form: expect.objectContaining({ attachmentMedia: "copy" }) }),
      );
      expect(result.current.form?.attachmentMedia).toBe("copy");
    }
  });

  it("resumes an Upload with the limit stored on the Import Run, not the server's current one", async () => {
    // The owner changed the limit after this run was staged and reviewed.
    // The files the person approved were measured against 7 MiB.
    getServerStateMock.mockResolvedValue({ asset_max_bytes: 100 * MIB });
    runMock.mockReset();
    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    const { result } = renderHook(() => useImportJob());
    await act(() =>
      result.current.startImport(
        { ...form({ attachmentMedia: "copy" }), assetMaxBytes: 7 * MIB },
        { sessionId: 9, stagingDir: "/home/sam/message-crate/staging-iphone" },
      ),
    );

    expect(getServerStateMock).not.toHaveBeenCalled();
    expect(invokePushMock).toHaveBeenCalledWith(
      expect.objectContaining({ asset_max_bytes: 7 * MIB, import_id: 9 }),
    );
  });

  it("resumes an interrupted Staging with the limit stored on the Import Run", async () => {
    getServerStateMock.mockResolvedValue({ asset_max_bytes: 100 * MIB });
    const { result } = renderHook(() => useImportJob());
    await act(() =>
      result.current.startImport(
        { ...form({ attachmentMedia: "copy" }), assetMaxBytes: 7 * MIB },
        undefined,
        { sessionId: 9, stagingDir: "/home/sam/message-crate/staging-iphone" },
      ),
    );

    expect(getServerStateMock).not.toHaveBeenCalled();
    expect(invokeExtractMock).toHaveBeenCalledWith(
      expect.objectContaining({ asset_max_bytes: 7 * MIB, resume: true }),
    );
  });

  it("ends the import without staging anything when the server's limit cannot be read", async () => {
    getServerStateMock.mockRejectedValue(new Error("Failed to fetch"));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));

    expect(createImportMock).not.toHaveBeenCalled();
    expect(invokeExtractMock).not.toHaveBeenCalled();
    expect(result.current.phase).toBe("done");
    expect(result.current.summaryView?.status).toBe("failed");
  });

  it("stops at the first gate instead of uploading", async () => {
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    expect(result.current.phase).toBe("staging_review");
    expect(invokePushMock).not.toHaveBeenCalled();
    expect(invokeTranscodeStagingMock).not.toHaveBeenCalled();
  });

  it("sends the form's zone with an iMazing extract, whose dates carry none", async () => {
    // Without it the exporter reads every date in the machine's zone (#689).
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport({ ...baseForm, source: "imazing" }));
    expect(invokeExtractMock).toHaveBeenCalledWith(
      expect.objectContaining({ source: "imazing", timezone: "America/New_York" }),
    );
  });

  it("sends no zone for a source whose dates carry their own", async () => {
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form()));
    expect(invokeExtractMock).toHaveBeenCalledWith(
      expect.not.objectContaining({ timezone: expect.anything() }),
    );
  });

  it("gives extract the mode the person chose under convert", async () => {
    // extract stages originals for it and records the choice for the Media stage.
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    expect(invokeExtractMock).toHaveBeenCalledWith(
      expect.objectContaining({ attachment_media: "convert" }),
    );
  });

  it("records the stage as it goes", async () => {
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    // No plan exists yet at either call — `setImportStage` still receives a
    // (harmlessly `undefined`) third argument; see `moveStage`.
    expect(setImportStageMock).toHaveBeenCalledWith(1, "write", undefined);
    expect(setImportStageMock).toHaveBeenCalledWith(1, "staging_review", undefined);
  });

  /** `setImportStage` rejects for `failing` and resolves for every other stage. */
  function failStageWrite(failing: string, times = Number.POSITIVE_INFINITY) {
    let left = times;
    setImportStageMock.mockImplementation((_id: number, stage: string) => {
      if (stage === failing && left > 0) {
        left -= 1;
        return Promise.reject(new Error("Failed to fetch"));
      }
      return Promise.resolve();
    });
  }

  function rowStatus(steps: { label: string; status: string }[], label: string) {
    return steps.find((step) => step.label === label)?.status;
  }

  it("does not start Staging when the server does not record the write stage", async () => {
    failStageWrite("write");
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));

    expect(invokeExtractMock).not.toHaveBeenCalled();
    expect(result.current.phase).toBe("done");
    expect(result.current.summaryView?.status).toBe("failed");
    expect(result.current.summaryView?.issues[0]?.reason).toMatch(/Failed to fetch/);
    expect(rowStatus(result.current.steps, "Staging")).toBe("error");
    // The run stays at the stage the server last recorded, so it is not completed.
    expect(completeImportMock).not.toHaveBeenCalled();
  });

  it("does not start the Media pass when the server does not record the transcode stage", async () => {
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    failStageWrite("media");
    await act(() => result.current.approve());

    expect(invokeTranscodeStagingMock).not.toHaveBeenCalled();
    expect(result.current.summaryView?.status).toBe("failed");
    expect(result.current.summaryView?.issues[0]?.reason).toMatch(/Failed to fetch/);
    expect(rowStatus(result.current.steps, "Media")).toBe("error");
    expect(completeImportMock).not.toHaveBeenCalled();
    expect(invokeDeleteStagingMock).not.toHaveBeenCalled();
  });

  it("does not start the Upload when the server does not record the pushing stage", async () => {
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    failStageWrite("upload");
    await act(() => result.current.approve());

    expect(invokePushMock).not.toHaveBeenCalled();
    expect(result.current.summaryView?.status).toBe("failed");
    expect(result.current.summaryView?.issues[0]?.reason).toMatch(/Failed to fetch/);
    expect(rowStatus(result.current.steps, "Upload")).toBe("error");
    expect(completeImportMock).not.toHaveBeenCalled();
    expect(invokeDeleteStagingMock).not.toHaveBeenCalled();
  });

  it("shows a failed staging_review write on the Staging Review, and approving writes it again first", async () => {
    failStageWrite("staging_review", 2);
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));

    expect(result.current.phase).toBe("staging_review");
    expect(result.current.reviewError).toMatch(/Failed to fetch/);

    // The second write fails too: the run stays at the review and uploads nothing.
    await act(() => result.current.approve());
    expect(result.current.phase).toBe("staging_review");
    expect(result.current.reviewError).toMatch(/Failed to fetch/);
    expect(invokePushMock).not.toHaveBeenCalled();

    // The third succeeds, and the Upload follows it.
    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    await act(() => result.current.approve());
    const stages = setImportStageMock.mock.calls.map(([, stage]) => stage);
    expect(stages.filter((stage) => stage === "staging_review")).toHaveLength(3);
    expect(stages.lastIndexOf("staging_review")).toBeLessThan(stages.indexOf("upload"));
    expect(result.current.reviewError).toBeNull();
    expect(invokePushMock).toHaveBeenCalled();
  });

  it("shows a failed media_review write on the Media Review, and approving writes it again first", async () => {
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );
    const approved = stagingSummary({ conversations: 5 });
    invokeSummarizeStagingMock.mockResolvedValueOnce(approved);
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    failStageWrite("media_review", 1);
    await act(() => result.current.approve());

    expect(result.current.phase).toBe("media_review");
    expect(result.current.reviewError).toMatch(/Failed to fetch/);

    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    await act(() => result.current.approve());
    const gateCalls = setImportStageMock.mock.calls.filter(([, stage]) => stage === "media_review");
    expect(gateCalls).toHaveLength(2);
    expect(gateCalls[1]).toEqual([1, "media_review", approved]);
    expect(result.current.reviewError).toBeNull();
    expect(invokePushMock).toHaveBeenCalled();
  });

  it("runs the media pass then stops at the second gate", async () => {
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approve());
    expect(invokeTranscodeStagingMock).toHaveBeenCalled();
    expect(result.current.phase).toBe("media_review");
    expect(invokePushMock).not.toHaveBeenCalled();
  });

  it("routes a progress event arriving during summarize to the staging row", async () => {
    // W6: `summarize_staging` (Rust) emits `extract:progress` with
    // `step: "check"` while it walks a big folder, but nothing used to
    // subscribe, so those events had nowhere to go and a huge folder's gate
    // looked frozen. The mocked `invokeSummarizeStaging` fires one here,
    // mid-call, through the callbacks `onExtractEvents` was given — exactly
    // what the real Tauri event stream would do.
    invokeSummarizeStagingMock.mockReset();
    invokeSummarizeStagingMock.mockImplementationOnce(async () => {
      lastExtractEventCallbacks?.onProgress?.({ step: "check", done: 50, total: 200 });
      return stagingSummary();
    });
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));

    expect(result.current.phase).toBe("staging_review");
    // Checking the staged attachments narrates the Staging row.
    expect(result.current.steps[0]?.detail).toBe("Checking attachments: 50/200");
  });

  it("narrates a setup step on the read row without marking it done", async () => {
    // An encrypted iPhone backup spends its first minutes deriving keys and
    // decrypting databases before a single message count arrives. The
    // exporter reports those as typed `setup` events; the read row shows the
    // step's label and stays active, so the screen never looks frozen and
    // never claims the backup is read before it is.
    let release: () => void = () => {};
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    // Replace beforeEach's canned extract run with one that reports a
    // setup step and then waits, so the row can be read mid-run.
    runMock.mockReset();
    runMock.mockImplementationOnce(
      async (
        fn: () => Promise<unknown>,
        _onLog?: (line: string) => void,
        onProgress?: (event: ImportProgressEvent) => void,
      ) => {
        await fn();
        onProgress?.({
          step: "setup",
          done: 5,
          total: 5,
          status: "Decrypting contacts database",
        });
        await held;
        return EXTRACT_RESULT;
      },
    );
    const { result } = renderHook(() => useImportJob());
    let started: Promise<void> = Promise.resolve();
    act(() => {
      started = result.current.startImport(form({ attachmentMedia: "copy" }));
    });
    await waitFor(() =>
      expect(result.current.steps[0]?.detail).toBe("Decrypting contacts database (5/5)"),
    );
    expect(result.current.steps[0]?.status).toBe("active");
    release();
    await act(() => started);
    expect(result.current.phase).toBe("staging_review");
  });

  it("keeps a line per stage on the Staging row while they run together", async () => {
    // Reading messages, copying attachments, and writing conversation files
    // report at the same time. With one shared line, each event replaced the
    // last, so the row flipped between them.
    let release: () => void = () => {};
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    runMock.mockReset();
    runMock.mockImplementationOnce(
      async (
        fn: () => Promise<unknown>,
        _onLog?: (line: string) => void,
        onProgress?: (event: ImportProgressEvent) => void,
      ) => {
        await fn();
        onProgress?.({ step: "setup", done: 5, total: 5, status: "Decrypting" });
        onProgress?.({ step: "parse", done: 10, total: 40 });
        onProgress?.({
          step: "attachments",
          done: 3,
          total: 9,
          bytes_done: 1024,
          bytes_total: 4096,
        });
        onProgress?.({ step: "prepare", done: 1, total: 4 });
        onProgress?.({
          step: "attachments",
          done: 4,
          total: 9,
          bytes_done: 2048,
          bytes_total: 4096,
        });
        await held;
        return EXTRACT_RESULT;
      },
    );
    const { result } = renderHook(() => useImportJob());
    let started: Promise<void> = Promise.resolve();
    act(() => {
      started = result.current.startImport(form({ attachmentMedia: "copy" }));
    });
    await waitFor(() =>
      expect(result.current.steps[0]?.detail).toBe(
        "Preparing conversations: 1/4\nReading messages: 10/40\nCopied attachments: 4/9 (2.0 KB / 4.0 KB)",
      ),
    );
    release();
    await act(() => started);
  });

  it("uploads straight from the first gate under copy, because there is no second one", async () => {
    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    await act(() => result.current.approve());
    expect(invokeTranscodeStagingMock).not.toHaveBeenCalled();
    expect(invokePushMock).toHaveBeenCalled();
  });

  it("carries the plan approved at Gate 1 into the pushing stage call under copy", async () => {
    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    const approved = stagingSummary({ conversations: 3 });
    invokeSummarizeStagingMock.mockResolvedValueOnce(approved);
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    await act(() => result.current.approve());

    expect(setImportStageMock).toHaveBeenCalledWith(1, "upload", approved);
  });

  it("recomputes the summary after the media pass rather than adjusting the old one", async () => {
    // Decision 39: the folder is the truth.
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    invokeSummarizeStagingMock.mockClear();
    await act(() => result.current.approve());
    expect(invokeSummarizeStagingMock).toHaveBeenCalledTimes(1);
  });

  it("writes transcode carrying the Gate-1 plan, then media_review carrying it too", async () => {
    // Important 4: a crash mid-pass must not leave summary_json null with
    // no baseline for a later resume — so the plan rides the "media"
    // stage call too, not only the one after it.
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );
    const approved = stagingSummary({ conversations: 5 });
    invokeSummarizeStagingMock.mockResolvedValueOnce(approved);
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approve());

    expect(setImportStageMock).toHaveBeenCalledWith(1, "media", approved);
    expect(setImportStageMock).toHaveBeenCalledWith(1, "media_review", approved);
  });

  it("declining closes the session and deletes the folder", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/staging/run-1");
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.cancelRun());
    expect(discardImportSessionMock).toHaveBeenCalledWith(1);
    expect(invokeDeleteStagingMock).toHaveBeenCalledWith({ staging_dir: "/staging/run-1" });
    expect(result.current.phase).toBe("form");
  });

  it("deletes the folder even when discarding the session fails", async () => {
    // Either half failing must not leave the other undone: a live session with
    // no folder blocks the next import, and a folder with no session is litter
    // nothing will ever clean up.
    discardImportSessionMock.mockRejectedValueOnce(new Error("offline"));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.cancelRun());
    expect(invokeDeleteStagingMock).toHaveBeenCalled();
  });

  it("still discards the session, and still returns to the form, even when deleting the folder fails", async () => {
    // The other direction of the same guarantee: a regression to sequential
    // discard-then-delete (each awaited without independent handling) would
    // let a rejected delete propagate out of cancelRun and skip
    // returnToForm — leaving the screen stuck on Gate 1 with a session the
    // server already considers discarded.
    invokeDeleteStagingMock.mockRejectedValueOnce(new Error("disk full"));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.cancelRun());
    expect(discardImportSessionMock).toHaveBeenCalledWith(1);
    expect(result.current.phase).toBe("form");
  });

  it("declines from Gate 2 the same way — closes the session and deletes the folder", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/staging/run-2");
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approve());
    expect(result.current.phase).toBe("media_review");

    await act(() => result.current.cancelRun());

    expect(discardImportSessionMock).toHaveBeenCalledWith(1);
    expect(invokeDeleteStagingMock).toHaveBeenCalledWith({ staging_dir: "/staging/run-2" });
    expect(result.current.phase).toBe("form");
  });

  it("a successful import deletes its staging directory once the server has recorded it", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/staging/run-3");
    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    await act(() => result.current.approve());

    expect(result.current.phase).toBe("done");
    expect(result.current.summaryView?.status).toBe("completed");
    expect(invokeDeleteStagingMock).toHaveBeenCalledWith({ staging_dir: "/staging/run-3" });
    // The server's record is written first; the folder goes after it.
    expect(completeImportMock.mock.invocationCallOrder[0]).toBeLessThan(
      invokeDeleteStagingMock.mock.invocationCallOrder[0] ?? 0,
    );
    // Nothing on the finished screen points at a folder that no longer exists.
    expect(result.current.stagingDir).toBeNull();
    expect(discardImportSessionMock).not.toHaveBeenCalled();
  });

  // #1233: a failed Upload is paused, not failed. It posts no /complete, so
  // the run stays at `upload` and the next visit to Import offers Resume
  // or Discard; the staged folder is what Resume sends from.
  it("pauses a failed Upload: no /complete, the run stays at pushing, and its folder stays", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/staging/run-4");
    runMock.mockImplementationOnce(
      runResult({ summary: "Push finished.", report: failedReport() }),
    );
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    await act(() => result.current.approve());

    expect(result.current.phase).toBe("done");
    expect(result.current.summaryView?.status).toBe("paused");
    expect(completeImportMock).not.toHaveBeenCalled();
    expect(discardImportSessionMock).not.toHaveBeenCalled();
    expect(setImportStageMock).toHaveBeenLastCalledWith(1, "upload", expect.anything());
    expect(invokeDeleteStagingMock).not.toHaveBeenCalled();
    expect(result.current.stagingDir).toBe("/staging/run-4");
  });

  it("pauses an Upload whose job failed outright, the same way", async () => {
    runMock.mockImplementationOnce(async (fn: () => Promise<unknown>) => {
      await fn();
      throw new Error("error sending request: connection refused");
    });
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    await act(() => result.current.approve());

    expect(result.current.summaryView?.status).toBe("paused");
    expect(result.current.summaryView?.issues).toContainEqual(
      expect.objectContaining({
        stage: "upload",
        reason: "error sending request: connection refused",
      }),
    );
    expect(completeImportMock).not.toHaveBeenCalled();
    expect(invokeDeleteStagingMock).not.toHaveBeenCalled();
  });

  it("pauses an Upload that left conversations unsent instead of finishing it and deleting them", async () => {
    // Some conversations landed, so the old verdict read the run as
    // completed_with_issues, completed it, and deleted the folder that held
    // the conversations never sent.
    runMock.mockImplementationOnce(
      runResult({
        summary: "Push finished.",
        report: okReport({
          ok: false,
          conversations_total: 10,
          conversations_ok: 7,
          conversations_failed: 1,
          conversations_cancelled: 2,
        }),
      }),
    );
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    await act(() => result.current.approve());

    expect(result.current.summaryView?.status).toBe("paused");
    expect(completeImportMock).not.toHaveBeenCalled();
    expect(invokeDeleteStagingMock).not.toHaveBeenCalled();
  });

  it("keeps a paused Upload's Import Errors in its folder, leaving out what the resume reports again", async () => {
    const stagingIssue: ImportIssueEvent = {
      kind: "skip",
      step: "parse",
      item: "IMG_1.HEIC",
      reason: "missing",
    };
    const attachmentSkip: ImportIssueEvent = {
      kind: "skip",
      step: "upload",
      item: "a.jsonl:attachments/big.mov",
      reason: "too large",
    };
    const conversationRow: ImportIssueEvent = {
      kind: "error",
      step: "upload",
      item: "b.jsonl",
      reason: "connection refused",
    };
    runMock.mockReset();
    runMock.mockImplementationOnce(runResultWithIssue(EXTRACT_RESULT, stagingIssue));
    runMock.mockImplementationOnce(
      runResultWithIssue(
        {
          summary: "Push finished.",
          report: okReport({
            ok: false,
            conversations_total: 2,
            conversations_ok: 1,
            conversations_failed: 1,
            assets_bytes: 4_096,
            results: [
              { file: "a.jsonl", status: "ok", messages: 5, attachments: 1 },
              {
                file: "b.jsonl",
                status: "failed",
                error: "connection refused",
                messages: 0,
                attachments: 0,
              },
            ],
          }),
        },
        attachmentSkip,
        conversationRow,
      ),
    );
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    await act(() => result.current.approve());

    expect(result.current.summaryView?.status).toBe("paused");
    const [{ staging_dir, record }] = saveRunRecordMock.mock.lastCall as [
      { staging_dir: string; record: Record<string, unknown> },
    ];
    expect(staging_dir).toBe("/home/sam/message-crate/staging-iphone");
    expect(record.issues).toEqual([
      { kind: "skip", stage: "staging", item: "IMG_1.HEIC", reason: "missing" },
      { kind: "skip", stage: "upload", item: "a.jsonl:attachments/big.mov", reason: "too large" },
    ]);
    expect(record.bytesUploaded).toBe(4_096);
    expect(record.filesSucceeded).toBe(1);
    expect(typeof record.uploadMs).toBe("number");
    expect(record.messagesParsed).toBe(8_000);
  });

  it("keeps the Staging issues in the folder when the run stops at the Staging Review", async () => {
    const stagingIssue: ImportIssueEvent = {
      kind: "error",
      step: "attachments",
      item: "IMG_2.HEIC",
      reason: "could not be decrypted",
    };
    runMock.mockReset();
    runMock.mockImplementationOnce(runResultWithIssue(EXTRACT_RESULT, stagingIssue));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));

    expect(result.current.phase).toBe("staging_review");
    const [{ record }] = saveRunRecordMock.mock.lastCall as [{ record: Record<string, unknown> }];
    expect(record.issues).toEqual([
      { kind: "error", stage: "staging", item: "IMG_2.HEIC", reason: "could not be decrypted" },
    ]);
  });

  it("completes a failed Staging as failed and deletes its folder, since nothing complete exists to upload", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/staging/run-6");
    runMock.mockReset();
    runMock.mockImplementationOnce(async (fn: () => Promise<unknown>) => {
      await fn();
      throw new Error("chat.db is not readable");
    });
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));

    expect(result.current.summaryView?.status).toBe("failed");
    expect(completeImportMock).toHaveBeenCalledWith(
      1,
      expect.objectContaining({ status: "failed" }),
    );
    expect(invokeDeleteStagingMock).toHaveBeenCalledWith({ staging_dir: "/staging/run-6" });
    expect(completeImportMock.mock.invocationCallOrder[0]).toBeLessThan(
      invokeDeleteStagingMock.mock.invocationCallOrder[0] ?? 0,
    );
    expect(result.current.stagingDir).toBeNull();
  });

  it("completes a failed Media stage as failed and deletes its folder", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/staging/run-7");
    runMock.mockImplementationOnce(async (fn: () => Promise<unknown>) => {
      await fn();
      throw new Error("ffmpeg exited with status 1");
    });
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approve());

    expect(result.current.summaryView?.status).toBe("failed");
    expect(completeImportMock).toHaveBeenCalledWith(
      1,
      expect.objectContaining({ status: "failed" }),
    );
    expect(invokeDeleteStagingMock).toHaveBeenCalledWith({ staging_dir: "/staging/run-7" });
  });

  it("keeps a failed Staging's folder when the server does not take its completion, since the run is still open", async () => {
    runMock.mockReset();
    runMock.mockImplementationOnce(async () => {
      throw new Error("chat.db is not readable");
    });
    completeImportMock.mockRejectedValue(new TypeError("Failed to fetch"));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));

    expect(completeImportMock).toHaveBeenCalled();
    expect(invokeDeleteStagingMock).not.toHaveBeenCalled();
  });

  it("a successful import still finishes when deleting the staging directory fails", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/staging/run-5");
    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    invokeDeleteStagingMock.mockRejectedValueOnce(new Error("permission denied"));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    await act(() => result.current.approve());

    expect(result.current.phase).toBe("done");
    expect(result.current.summaryView?.status).toBe("completed");
    // The folder is still there, so the screen keeps pointing at it.
    expect(result.current.stagingDir).toBe("/staging/run-5");
  });

  it("approving at Gate 2 writes pushing carrying the recomputed summary, not Gate 1's", async () => {
    // Decision 15: the diff at Gate 2 is against what was approved at Gate
    // 1, but what gets approved when Gate 2 itself is approved is the
    // summary Gate 2 is showing — the recomputed one, not the original.
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );
    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    const gate1Approved = stagingSummary({ conversations: 1 });
    const recomputed = stagingSummary({ conversations: 1, attachments: 4 });
    invokeSummarizeStagingMock.mockResolvedValueOnce(gate1Approved);
    invokeSummarizeStagingMock.mockResolvedValueOnce(recomputed);

    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approve()); // Gate 1 -> media pass -> Gate 2
    expect(result.current.phase).toBe("media_review");

    await act(() => result.current.approve()); // Gate 2 -> pushing

    expect(setImportStageMock).toHaveBeenCalledWith(1, "upload", recomputed);
    expect(setImportStageMock).not.toHaveBeenCalledWith(1, "upload", gate1Approved);
  });

  it("keeps what Staging made beside what Media made, with Media's failed count", async () => {
    runMock.mockImplementationOnce(
      runResult({
        summary: "Transcode finished.",
        transcode: {
          converted: 5,
          skipped: 0,
          too_large: 1,
          failed: 2,
          missing: 0,
          repointed: 0,
          bytes_before: 100,
          bytes_after: 40,
        },
      }),
    );
    const staged = stagingSummary({ conversations: 1, attachmentBytes: 100 });
    const afterMedia = stagingSummary({ conversations: 1, attachmentBytes: 40 });
    invokeSummarizeStagingMock.mockResolvedValueOnce(staged);
    invokeSummarizeStagingMock.mockResolvedValueOnce(afterMedia);

    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    expect(result.current.mediaSummary).toBeNull();
    await act(() => result.current.approve());

    expect(result.current.phase).toBe("media_review");
    expect(result.current.stagingSummary).toEqual(staged);
    expect(result.current.mediaSummary).toEqual(afterMedia);
    expect(result.current.mediaFailedCount).toBe(2);
  });

  it("pauses a failed push through Gate 2 the same way copy mode does through Gate 1", async () => {
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );
    runMock.mockImplementationOnce(
      runResult({ summary: "Push finished.", report: failedReport() }),
    );
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approve());
    expect(result.current.phase).toBe("media_review");

    await act(() => result.current.approve());

    expect(result.current.phase).toBe("done");
    expect(result.current.summaryView?.status).toBe("paused");
  });

  it("unwedges on a cancelled media pass instead of freezing the screen", async () => {
    // Critical: transcode_staging used to end a cancelled pass quietly (an
    // extract:log line, Ok(())) with no extract:finished and no
    // extract:error, so awaitTauriJob's promise never settled and the
    // screen was stuck. It now reports through extract:error like any other
    // failure, so run() rejects here exactly as it would for a real error.
    runMock.mockImplementationOnce(async (fn: () => Promise<unknown>) => {
      await fn();
      throw new Error("cancelled");
    });
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approve());

    expect(invokePushMock).not.toHaveBeenCalled();
    expect(result.current.phase).toBe("done");
    expect(result.current.summaryView?.status).toBe("cancelled");
    // The session stays wherever the run actually got to — "media" —
    // never advanced to a stage the cancelled run never reached.
    expect(setImportStageMock).not.toHaveBeenCalledWith(1, "media_review", expect.anything());
    expect(setImportStageMock).not.toHaveBeenCalledWith(1, "upload", expect.anything());
  });

  it("does not complete the session on a cancelled media pass, so it stays resumable", async () => {
    // Decision 36 routes a cancellation mid-transcode to the same recovery
    // as a crash at that stage; decision 37 says only an explicit discard
    // ends a waiting session. Posting /complete would free the one-active-
    // session slot and drop the session out of GET /v1/imports?status=running,
    // stranding the staged folder with no session left to resume it
    // through — even though the "cancelled" outcome is still shown locally.
    runMock.mockImplementationOnce(async (fn: () => Promise<unknown>) => {
      await fn();
      throw new Error("cancelled");
    });
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approve());

    expect(result.current.summaryView?.status).toBe("cancelled");
    expect(completeImportMock.mock.calls.some(([id]) => id === 1)).toBe(false);
  });

  it("still completes the session as failed when the media pass genuinely fails", async () => {
    // Unlike a cancellation, a broken ffmpeg (or any other real failure)
    // must not lock the account out of importing — the run still completes
    // and frees the slot, same as before.
    runMock.mockRejectedValueOnce(new Error("ffmpeg exited with status 1"));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approve());

    expect(result.current.summaryView?.status).toBe("failed");
    const completeCall = completeImportMock.mock.calls.find(([id]) => id === 1);
    expect(completeCall).toBeDefined();
    const [, body] = completeCall as [string, Record<string, unknown>];
    expect(body.status).toBe("failed");
  });

  it("does not complete the session on a cancelled extract, so the copy can be picked up", async () => {
    // Decision 36 gives a cancellation the same recovery as a crash at that
    // stage, and the write stage is resumable now: the conversations already
    // copied are real work. Completing here would free the one-active-session
    // slot and strand them with no session left to resume through.
    runMock.mockReset();
    runMock.mockImplementationOnce(async (fn: () => Promise<unknown>) => {
      await fn();
      throw new Error("cancelled");
    });
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));

    expect(result.current.summaryView?.status).toBe("cancelled");
    expect(completeImportMock.mock.calls.some(([id]) => id === 1)).toBe(false);
  });

  it("does not start the extract when Cancel is pressed while the run is being created", async () => {
    // A Cancel sent while no job runs stops nothing, and invokeExtract starts
    // its job with a cancel flag of its own, so the run must not start it.
    let releaseCreate: (value: { id: number }) => void = () => {};
    createImportMock.mockReset();
    createImportMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          releaseCreate = resolve;
        }),
    );
    const { result } = renderHook(() => useImportJob());
    let started: Promise<void> = Promise.resolve();
    act(() => {
      started = result.current.startImport(form({ attachmentMedia: "copy" }));
    });
    await waitFor(() => expect(createImportMock).toHaveBeenCalled());
    await act(() => result.current.cancel());
    releaseCreate({ id: 1 });
    await act(() => started);

    expect(invokeExtractMock).not.toHaveBeenCalled();
    expect(result.current.summaryView?.status).toBe("cancelled");
    expect(completeImportMock.mock.calls.some(([id]) => id === 1)).toBe(false);
  });

  it("does not start the upload when Cancel is pressed while the pushing stage is written", async () => {
    let releaseStage: () => void = () => {};
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    setImportStageMock.mockImplementation((_id: number, stage: string) =>
      stage === "upload"
        ? new Promise<void>((resolve) => {
            releaseStage = resolve;
          })
        : Promise.resolve(),
    );
    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    let approved: Promise<void> = Promise.resolve();
    act(() => {
      approved = result.current.approve();
    });
    await waitFor(() =>
      expect(setImportStageMock).toHaveBeenCalledWith(1, "upload", expect.anything()),
    );
    await act(() => result.current.cancel());
    releaseStage();
    await act(() => approved);

    expect(invokePushMock).not.toHaveBeenCalled();
    // The Upload is paused, not ended: the run stays at `upload` with its folder.
    expect(result.current.summaryView?.status).toBe("paused");
    expect(completeImportMock).not.toHaveBeenCalled();
    expect(invokeDeleteStagingMock).not.toHaveBeenCalled();
  });

  it("pauses an Upload the cancel flag stopped: no /complete, and the staged files stay", async () => {
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    // What run.rs reports when the cancel flag stops `drive` after 200 of 681.
    runMock.mockImplementationOnce(
      runResult({
        summary: "Push complete",
        report: okReport({
          ok: false,
          cancelled: true,
          conversations_ok: 200,
          conversations_total: 681,
          conversations_failed: 0,
        }),
      }),
    );
    await act(() => result.current.approve());

    expect(result.current.summaryView?.status).toBe("paused");
    expect(completeImportMock).not.toHaveBeenCalled();
    expect(invokeDeleteStagingMock).not.toHaveBeenCalled();
    expect(result.current.stagingDir).toBe("/home/sam/message-crate/staging-iphone");
  });

  it("pauses an Upload that stopped short without a cancel, and keeps the staged files", async () => {
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    runMock.mockImplementationOnce(
      runResult({
        summary: "Push complete",
        report: okReport({
          ok: false,
          conversations_ok: 200,
          conversations_total: 681,
          conversations_failed: 0,
        }),
      }),
    );
    await act(() => result.current.approve());

    expect(result.current.summaryView?.status).toBe("paused");
    expect(completeImportMock).not.toHaveBeenCalled();
    expect(invokeDeleteStagingMock).not.toHaveBeenCalled();
  });

  it("keeps the staged files when Message Crate does not take the run's completion", async () => {
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    completeImportMock.mockRejectedValue(new TypeError("Failed to fetch"));
    await act(() => result.current.approve());

    expect(completeImportMock).toHaveBeenCalled();
    // The run is left at `upload`; its resume needs this folder.
    expect(invokeDeleteStagingMock).not.toHaveBeenCalled();
    expect(result.current.stagingDir).toBe("/home/sam/message-crate/staging-iphone");
    // Not finished, so not shown as an import with a Saved Search and a
    // Contact Group: it is paused, and the next visit resumes it.
    expect(result.current.summaryView?.status).toBe("paused");
    expect(result.current.summaryView?.issues).toContainEqual(
      expect.objectContaining({
        kind: "error",
        stage: "upload",
        reason: "Message Crate didn't record the import as finished: Failed to fetch",
      }),
    );
  });

  it("leaves the run's message and attachment counts to the server", async () => {
    // A resumed Upload's push report counts only what the resume sent, so a
    // count from the client would record a run that completed after a
    // resume as holding no messages, and the server makes no Saved Search
    // for that.
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    await act(() => result.current.approve());

    expect(completeImportMock).toHaveBeenCalledTimes(1);
    const body = completeImportMock.mock.calls[0]?.[1] as Record<string, unknown>;
    expect(body).not.toHaveProperty("message_count");
    expect(body).not.toHaveProperty("attachment_count");
  });

  it("sends Cancel again once a job has started, when it was pressed while the job was starting", async () => {
    // A Cancel that reached the desktop side before the job started stopped
    // nothing, because no job was running yet, so it has to be sent again.
    let releasePush: () => void = () => {};
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    invokePushMock.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          releasePush = resolve;
        }),
    );
    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    let approved: Promise<void> = Promise.resolve();
    act(() => {
      approved = result.current.approve();
    });
    await waitFor(() => expect(invokePushMock).toHaveBeenCalled());
    await act(() => result.current.cancel());
    expect(cancelMock).toHaveBeenCalledTimes(1);
    releasePush();
    await act(() => approved);

    expect(cancelMock).toHaveBeenCalledTimes(2);
  });

  it("does not carry a Cancel from one run into the next", async () => {
    runMock.mockReset();
    runMock.mockImplementationOnce(async (fn: () => Promise<unknown>) => {
      await fn();
      throw new Error("cancelled");
    });
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    await act(() => result.current.cancel());
    expect(result.current.summaryView?.status).toBe("cancelled");

    invokeExtractMock.mockClear();
    runMock.mockImplementationOnce(runResult(EXTRACT_RESULT));
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    expect(invokeExtractMock).toHaveBeenCalledTimes(1);
  });

  it("still completes the session as failed when the extract genuinely fails", async () => {
    // A real failure must not lock the account out of importing: the run
    // completes and frees the slot, and restart-with-settings covers it.
    runMock.mockReset();
    runMock.mockImplementationOnce(async (fn: () => Promise<unknown>) => {
      await fn();
      throw new Error("chat.db is not readable");
    });
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));

    const completeCall = completeImportMock.mock.calls.find(([id]) => id === 1);
    expect(completeCall).toBeDefined();
    const [, body] = completeCall as [string, Record<string, unknown>];
    expect(body.status).toBe("failed");
  });

  it("does not strand the folder when the post-extract summarize fails right after a successful extract", async () => {
    // W8: extract succeeds and stages hours of work, but the summarize call
    // that follows it (on the way to Gate 1) fails. Routing that through
    // finishImport would post /complete and end the session, orphaning the
    // staged folder with no way back to it. This must behave like the
    // gate-resume recompute failure instead: no /complete, no phase "done",
    // back to the form with the error on resumeError so the next visit's
    // resume check re-finds the same session (stage staging_review) and
    // offers it again.
    invokeSummarizeStagingMock.mockRejectedValueOnce(new Error("disk full"));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));

    expect(result.current.phase).toBe("form");
    expect(result.current.resumeError).toBe("disk full");
    const completeCall = completeImportMock.mock.calls.find(([id]) => id === 1);
    expect(completeCall).toBeUndefined();
    // The stage write that already happened before the failing summarize
    // call stands -- nothing here regresses or overwrites it.
    expect(setImportStageMock).toHaveBeenCalledWith(1, "staging_review", undefined);
  });

  it("extract's own failure is unaffected by the summarize fix -- it still completes as failed", async () => {
    runMock.mockReset();
    runMock.mockImplementationOnce(async () => {
      throw new Error("backup file not found");
    });
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));

    expect(result.current.phase).toBe("done");
    expect(result.current.summaryView?.status).toBe("failed");
    const completeCall = completeImportMock.mock.calls.find(([id]) => id === 1);
    expect(completeCall).toBeDefined();
  });

  it("a failed recompute after a successful media pass is a failed import, not an unhandled rejection", async () => {
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );
    // The first summarize call is `startImport`'s own, on the way to Gate 1
    // — that one must succeed so this pins the *media pass's* recompute
    // failure specifically (W8 gave the Gate-1-bound call its own, milder
    // failure path: see the "does not strand the folder" test above).
    invokeSummarizeStagingMock.mockResolvedValueOnce(stagingSummary());
    invokeSummarizeStagingMock.mockRejectedValueOnce(new Error("disk full"));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approve());

    expect(invokePushMock).not.toHaveBeenCalled();
    expect(result.current.phase).toBe("done");
    expect(result.current.summaryView?.status).toBe("failed");
    expect(setImportStageMock).not.toHaveBeenCalledWith(1, "media_review", expect.anything());
  });

  it("does not run the media pass twice on a double click", async () => {
    let resolveTranscode!: (value: TauriJobResult) => void;
    const pending = new Promise<TauriJobResult>((resolve) => {
      resolveTranscode = resolve;
    });
    // Deliberately left unresolved: lets the two approve() calls below
    // race while the pass is still "running".
    runMock.mockImplementationOnce(async (fn: () => Promise<unknown>) => {
      await fn();
      return pending;
    });
    // A fallback that keeps calling through, so if the double-click guard
    // were ever removed, a genuine second (or third) run() call would show
    // up as a genuine second call to invokeTranscodeStagingMock below —
    // without this, the mock's one-time queue would just exhaust and return
    // `undefined` without invoking anything, and the guard could be deleted
    // without this test noticing.
    runMock.mockImplementation(runResult({ summary: "Transcode finished.", transcode: undefined }));

    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(async () => {
      void result.current.approve();
      void result.current.approve();
      resolveTranscode({ summary: "Transcode finished.", transcode: undefined });
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    expect(invokeTranscodeStagingMock).toHaveBeenCalledTimes(1);
    expect(runMock).toHaveBeenCalledTimes(2); // extract, then exactly one media pass
  });

  it("a failed media pass is a failed import, not a silent skip to upload", async () => {
    runMock.mockImplementationOnce(async (fn: () => Promise<unknown>) => {
      await fn();
      throw new Error("ffmpeg missing");
    });
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approve());
    expect(invokePushMock).not.toHaveBeenCalled();
    expect(result.current.phase).toBe("done");
    expect(result.current.summaryView?.status).toBe("failed");
  });

  it("carries a report where every conversation failed through to a paused summary and no /complete", async () => {
    runMock.mockImplementationOnce(
      runResult({ summary: "Push finished.", report: failedReport() }),
    );
    const { result } = renderHook(() => useImportJob());

    await act(() => result.current.startImport(baseForm));
    await act(() => result.current.approve());

    // The wiring under test: the hook's own verdict, not importOutcome's.
    expect(result.current.summaryView?.status).toBe("paused");
    expect(result.current.phase).toBe("done");
    expect(completeImportMock.mock.calls.find(([id]) => id === 1)).toBeUndefined();
  });

  it("sends the server whole milliseconds, which is what it stores", async () => {
    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(baseForm));
    await act(() => result.current.approve());

    const body = completeImportMock.mock.calls[0]?.[1] as Record<string, unknown>;
    for (const field of ["duration_ms", "parse_ms", "attachments_ms", "prepare_ms", "upload_ms"]) {
      expect(Number.isInteger(body[field]), field).toBe(true);
    }
  });

  it("records the staging folder and device on the session it creates", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/home/u/message-crate/staging-260830");
    invokePathStatMock.mockResolvedValue({
      exists: true,
      isFile: false,
      isDirectory: true,
      sizeBytes: 4096,
      modifiedUnixMs: 1_756_512_000_000,
    });

    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(baseForm));

    const createCall = createImportMock.mock.calls[0];
    expect(createCall).toBeDefined();
    const body = createCall?.[0] as Record<string, unknown>;
    expect(body.stage).toBe("parse");
    expect(body.device_id).toEqual(expect.any(String));
    expect(body.staging_dir).toBe("/home/u/message-crate/staging-260830");
    expect(body.form).toMatchObject({ source: "imessage-ios" });
  });

  it("keeps the backup password out of the stored form snapshot", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/tmp/staging");
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport({ ...baseForm, backupPassword: "hunter2" }));

    const body = createImportMock.mock.calls[0]?.[0] as Record<string, unknown>;
    expect(JSON.stringify(body.form)).not.toContain("hunter2");
    expect(body.form).not.toHaveProperty("backupPassword");
    // The resume has to know to ask for it again (#966).
    expect(body.form).toMatchObject({ backupPasswordGiven: true, whatsappKeyGiven: false });
  });

  // A WhatsApp import from an encrypted iPhone backup reads the backup with
  // the same password, so its resume has to ask for it again too (#941).
  it("records that a WhatsApp iPhone import was given the backup password", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/tmp/staging");
    const { result } = renderHook(() => useImportJob());
    await act(() =>
      result.current.startImport({
        ...baseForm,
        source: "whatsapp-ios",
        backupPassword: "hunter2",
      }),
    );

    const body = createImportMock.mock.calls[0]?.[0] as Record<string, unknown>;
    expect(JSON.stringify(body.form)).not.toContain("hunter2");
    expect(body.form).toMatchObject({ backupPasswordGiven: true, whatsappKeyGiven: false });
  });

  it("keeps the WhatsApp key out of the stored form snapshot", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/tmp/staging");
    const { result } = renderHook(() => useImportJob());
    await act(() =>
      result.current.startImport({
        ...baseForm,
        source: "whatsapp-android",
        whatsappKey: "0123abcd4567",
        whatsappOwnerPhone: "+15551234567",
      }),
    );

    const body = createImportMock.mock.calls[0]?.[0] as Record<string, unknown>;
    expect(JSON.stringify(body.form)).not.toContain("0123abcd4567");
    expect(body.form).not.toHaveProperty("whatsappKey");
    expect(body.form).toMatchObject({ backupPasswordGiven: false, whatsappKeyGiven: true });
  });

  it("records that no password or key was given when the form had none", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/tmp/staging");
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(baseForm));

    const body = createImportMock.mock.calls[0]?.[0] as Record<string, unknown>;
    expect(body.form).toMatchObject({ backupPasswordGiven: false, whatsappKeyGiven: false });
  });

  it("moves the session to pushing before the upload starts", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/tmp/staging");
    runMock.mockImplementationOnce(
      runResult({ summary: "Push finished.", report: failedReport() }),
    );

    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(baseForm));
    await act(() => result.current.approve());

    const stageCall = setImportStageMock.mock.calls.find(([, stage]) => stage === "upload");
    expect(stageCall).toBeDefined();
    expect(stageCall?.[0]).toBe(1);
  });

  it("assembles a 3-row step list in convert mode, stopping at the gate with the media row still pending", async () => {
    // Pins the mode-dependent assembly stepsFor/stepIndexFor exist for: this
    // hook does not run the media pass until Gate 1 is approved, so the row
    // must sit pending, not silently vanish or get marked done.
    resolveImportStagingDirMock.mockResolvedValue("/tmp/staging");

    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport({ ...baseForm, attachmentMedia: "convert" }));

    expect(result.current.phase).toBe("staging_review");
    expect(result.current.steps.map((s) => s.label)).toEqual(["Staging", "Media", "Upload"]);
    expect(result.current.steps[1]?.status).toBe("pending");
    expect(result.current.steps[2]?.status).toBe("pending");
  });

  it("continues the convert-mode step list through the media pass into Gate 2", async () => {
    // Task 7 pinned the media row sitting pending after extract; this
    // continues the same run through review: active while the pass runs,
    // done once it finishes.
    resolveImportStagingDirMock.mockResolvedValue("/tmp/staging");
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );

    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport({ ...baseForm, attachmentMedia: "convert" }));
    await act(() => result.current.approve());

    expect(result.current.phase).toBe("media_review");
    expect(result.current.steps[1]?.status).toBe("done");
  });

  it("says the staging row was Copied under convert, since extract only stages originals now", async () => {
    // Important 5: extract stages originals under convert/compress too
    // (ruling 3) — the staging row must say what extract actually did, not
    // what the user ultimately asked for. The media row (index 1) still
    // tells the convert/compress story once the pass itself runs.
    resolveImportStagingDirMock.mockResolvedValue("/tmp/staging");
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport({ ...baseForm, attachmentMedia: "convert" }));

    expect(result.current.steps[0]?.detail).toMatch(/^Copied /);
    expect(result.current.steps[0]?.detail).not.toMatch(/^Converted /);
  });

  it("never probes ffmpeg tools under copy mode, which never needs them", async () => {
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    expect(probeFfmpegToolsMock).not.toHaveBeenCalled();
    expect(result.current.mediaToolsMissing).toBe(false);
  });

  it("flags missing ffmpeg tools at Gate 1 under convert", async () => {
    probeFfmpegToolsMock.mockResolvedValue({
      ok: false,
      ffmpeg_path: null,
      ffprobe_path: null,
      error: "ffmpeg not found",
    });
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    expect(result.current.mediaToolsMissing).toBe(true);
  });

  describe("identity check", () => {
    function imessageForm() {
      return form();
    }

    function sbrForm() {
      return { ...baseForm, source: "sms-backup-restore" };
    }

    it("stops at identity_stop when nothing the backup sent from is on the profile", async () => {
      invokeImessageBackupIdentitiesMock.mockResolvedValue(["+15550001111"]);
      loadAccountProfileMock.mockResolvedValue({ phones: ["+15559999999"], emails: [] });
      const { result } = renderHook(() => useImportJob());
      await act(async () => {
        await result.current.startImport(imessageForm());
      });
      expect(result.current.phase).toBe("identity_stop");
      expect(result.current.sourceIdentities).toEqual(["+15550001111"]);
      // Nothing was created: no session POST, no extract.
      expect(createImportMock).not.toHaveBeenCalled();
      expect(invokeExtractMock).not.toHaveBeenCalled();
    });

    it("continueAfterIdentityStop proceeds and sends the identities on the session", async () => {
      invokeImessageBackupIdentitiesMock.mockResolvedValue(["+15550001111"]);
      loadAccountProfileMock.mockResolvedValue({ phones: ["+15559999999"], emails: [] });
      const { result } = renderHook(() => useImportJob());
      await act(async () => {
        await result.current.startImport(imessageForm());
      });
      await act(async () => {
        await result.current.continueAfterIdentityStop();
      });
      expect(createImportMock).toHaveBeenCalledWith(
        expect.objectContaining({ source_identities: ["+15550001111"] }),
      );
    });

    it("cancelIdentityStop returns to the form with nothing created", async () => {
      invokeImessageBackupIdentitiesMock.mockResolvedValue(["+15550001111"]);
      loadAccountProfileMock.mockResolvedValue({ phones: [], emails: [] });
      const { result } = renderHook(() => useImportJob());
      await act(async () => {
        await result.current.startImport(imessageForm());
      });
      act(() => {
        result.current.cancelIdentityStop();
      });
      expect(result.current.phase).toBe("form");
      expect(createImportMock).not.toHaveBeenCalled();
    });

    it("proceeds without a stop when an identity matches, sending the list", async () => {
      invokeImessageBackupIdentitiesMock.mockResolvedValue(["+15550001111"]);
      loadAccountProfileMock.mockResolvedValue({ phones: ["+1 555 000 1111"], emails: [] });
      const { result } = renderHook(() => useImportJob());
      await act(async () => {
        await result.current.startImport(imessageForm());
      });
      expect(result.current.phase).not.toBe("identity_stop");
      expect(createImportMock).toHaveBeenCalledWith(
        expect.objectContaining({ source_identities: ["+15550001111"] }),
      );
    });

    it("fails open when the probe errors", async () => {
      invokeImessageBackupIdentitiesMock.mockRejectedValue(new Error("locked"));
      loadAccountProfileMock.mockResolvedValue({ phones: [], emails: [] });
      const { result } = renderHook(() => useImportJob());
      await act(async () => {
        await result.current.startImport(imessageForm());
      });
      expect(result.current.phase).not.toBe("identity_stop");
    });

    it("does not probe non-iMessage sources", async () => {
      const { result } = renderHook(() => useImportJob());
      await act(async () => {
        await result.current.startImport(sbrForm());
      });
      expect(invokeImessageBackupIdentitiesMock).not.toHaveBeenCalled();
    });

    it("resume_write reaches Gate 1 with the session's stored identities, without re-probing", async () => {
      const { result } = renderHook(() => useImportJob());
      await act(async () => {
        await result.current.startImport(imessageForm(), undefined, {
          sessionId: 42,
          stagingDir: "/home/u/message-crate/staging-260830",
          identities: ["+15550001111"],
        });
      });
      expect(invokeImessageBackupIdentitiesMock).not.toHaveBeenCalled();
      expect(result.current.phase).toBe("staging_review");
      expect(result.current.sourceIdentities).toEqual(["+15550001111"]);
    });

    it("resume_write reads the backup with the password the form carries", async () => {
      // The Resume Import panel puts what was typed into the form (#966).
      const { result } = renderHook(() => useImportJob());
      await act(async () => {
        await result.current.startImport(
          { ...imessageForm(), backupPassword: "hunter2" },
          undefined,
          {
            sessionId: 42,
            stagingDir: "/home/u/message-crate/staging-260830",
            identities: null,
          },
        );
      });
      expect(invokeExtractMock).toHaveBeenCalledWith(
        expect.objectContaining({ resume: true, backup_password: "hunter2" }),
      );
    });

    it("guards a double-click during the probe: probes once and creates at most one session", async () => {
      const { result } = renderHook(() => useImportJob());
      await act(async () => {
        await Promise.all([
          result.current.startImport(imessageForm()),
          result.current.startImport(imessageForm()),
        ]);
      });
      expect(invokeImessageBackupIdentitiesMock).toHaveBeenCalledTimes(1);
      expect(createImportMock.mock.calls).toHaveLength(1);
    });
  });
});

describe("useImportJob resume path", () => {
  beforeEach(() => {
    resetImportRun();
    runMock.mockReset();
    // A resumed run only ever calls run() once, for the push — and, like
    // the wiring tests above, must actually call the invoke function so
    // invokePush's args can be inspected.
    runMock.mockImplementation(runResult({ summary: "Push finished.", report: okReport() }));
    cancelMock.mockReset();
    createImportMock.mockReset();
    createImportMock.mockResolvedValue({ id: 1 });
    completeImportMock.mockReset();
    completeImportMock.mockResolvedValue({});
    resolveImportStagingDirMock.mockReset();
    invokePathStatMock.mockReset();
    invokePathStatMock.mockResolvedValue(null);
    invokePushMock.mockReset();
    readRunRecordMock.mockReset();
    saveRunRecordMock.mockReset();
    setImportStageMock.mockReset();
    setImportStageMock.mockResolvedValue(undefined);
    discardImportSessionMock.mockReset();
  });

  it("passes the resumed session id and staging dir through to invokePush", async () => {
    const { result } = renderHook(() => useImportJob());
    await act(async () => {
      await result.current.startImport(baseForm, {
        sessionId: 99,
        stagingDir: "/home/u/message-crate/staging-260830",
      });
    });

    expect(invokePushMock).toHaveBeenCalledTimes(1);
    expect(invokePushMock).toHaveBeenCalledWith(
      expect.objectContaining({
        import_id: 99,
        input_dir: "/home/u/message-crate/staging-260830",
      }),
    );
  });

  it("skips staging resolve, session create, and extract when resuming a push", async () => {
    // A push that pauses again, so the run, and its folder, stay on screen.
    runMock.mockImplementation(runResult({ summary: "Push finished.", report: failedReport() }));
    const { result } = renderHook(() => useImportJob());

    await act(async () => {
      await result.current.startImport(baseForm, {
        sessionId: 99,
        stagingDir: "/home/u/message-crate/staging-260830",
      });
    });

    expect(resolveImportStagingDirMock).not.toHaveBeenCalled();
    expect(invokePathStatMock).not.toHaveBeenCalled();
    expect(createImportMock.mock.calls.length > 0).toBe(false);
    expect(runMock).toHaveBeenCalledTimes(1); // push only, no extract
    expect(result.current.stagingDir).toBe("/home/u/message-crate/staging-260830");
    expect(result.current.importSessionId).toBe(99);
  });

  it("marks the staging steps already staged and moves the session to pushing without a plan", async () => {
    // baseForm uses attachmentMedia "copy", which has no Media stage:
    // Staging and Upload, two rows, not three.
    // Nothing was ever gated on a resumed run, so there is no approved plan
    // to carry — `setImportStage` still receives a third argument, but it's
    // `undefined`, which reaches the server identically to omitting it
    // entirely (JSON.stringify drops it).
    const { result } = renderHook(() => useImportJob());

    await act(async () => {
      await result.current.startImport(baseForm, {
        sessionId: 99,
        stagingDir: "/home/u/message-crate/staging-260830",
      });
    });

    expect(setImportStageMock).toHaveBeenCalledWith(99, "upload", undefined);

    expect(result.current.steps).toHaveLength(2);
    for (const step of result.current.steps.slice(0, 1)) {
      expect(step.status).toBe("done");
      expect(step.detail).toBe("Already staged");
      expect(step.durationMs).toBeUndefined();
    }
    expect(result.current.steps[1]).toMatchObject({ label: "Upload" });
  });

  it("resumed push: a skip matching the stored plan's forecast completes clean", async () => {
    // B3: `resume.approved` — the plan parsed from the session's stored
    // `summary` — must reach `runPush`/`finishImport` on the resume path, or
    // an expected omission (already flagged `probably_too_big` at the last
    // gate) reads as unexplained and demotes an honest "completed" verdict to
    // "completed_with_issues" for exactly the interrupted-and-resumed case.
    runMock.mockImplementation(
      runResultWithIssue(
        { summary: "Push finished.", report: okReport() },
        { kind: "skip", step: "upload", item: "attachments/big.mov", reason: "too_large" },
      ),
    );
    const approved = stagingSummary({
      forecasts: [
        {
          path: "attachments/big.mov",
          name: "big.mov",
          sizeBytes: 900_000_000,
          estimateBytes: 900_000_000,
          verdict: "probably_too_big",
        },
      ],
    });
    const { result } = renderHook(() => useImportJob());

    await act(async () => {
      await result.current.startImport(baseForm, {
        sessionId: 99,
        stagingDir: "/home/u/message-crate/staging-260830",
        approved,
      });
    });

    expect(result.current.summaryView?.status).toBe("completed");
  });

  it("resumed push with no stored plan: the same skip reads as unexplained", async () => {
    // The control case: without `approved`, the identical skip has nothing
    // to be diffed against and must still demote the verdict — pinning that
    // the fix is the plan reaching runPush, not a change to importOutcome.
    runMock.mockImplementation(
      runResultWithIssue(
        { summary: "Push finished.", report: okReport() },
        { kind: "skip", step: "upload", item: "attachments/big.mov", reason: "too_large" },
      ),
    );
    const { result } = renderHook(() => useImportJob());

    await act(async () => {
      await result.current.startImport(baseForm, {
        sessionId: 99,
        stagingDir: "/home/u/message-crate/staging-260830",
      });
    });

    expect(result.current.summaryView?.status).toBe("completed_with_issues");
  });

  it("completes a resumed Upload with the earlier parts' Import Errors, times, bytes and counts", async () => {
    // The first part paused and left its record in the folder. Without it,
    // the run's record would hold only what the resumed part did.
    const carriedIssue = { kind: "skip", stage: "staging", item: "IMG_1.HEIC", reason: "missing" };
    readRunRecordMock.mockResolvedValue({
      issues: [carriedIssue],
      durationMs: 60_000,
      parseMs: 1_000,
      attachmentsMs: 2_000,
      prepareMs: 3_000,
      uploadMs: 40_000,
      bytesUploaded: 10_000,
      filesParsed: 3,
      messagesParsed: 30,
      filesSucceeded: 2,
      messagesAttempted: 20,
      messagesInserted: 20,
      messagesDeduped: 0,
      attachmentsUploaded: 4,
    });
    runMock.mockImplementation(
      runResult({
        summary: "Push finished.",
        report: okReport({
          conversations_total: 3,
          conversations_ok: 1,
          conversations_skipped: 2,
          messages_attempted: 10,
          messages_inserted: 10,
          assets_uploaded: 1,
          assets_bytes: 500,
        }),
      }),
    );
    const { result } = renderHook(() => useImportJob());
    await act(async () => {
      await result.current.startImport(baseForm, {
        sessionId: 99,
        stagingDir: "/home/u/message-crate/staging-260830",
      });
    });

    expect(readRunRecordMock).toHaveBeenCalledWith({
      staging_dir: "/home/u/message-crate/staging-260830",
    });
    const [, body] = completeImportMock.mock.calls[0] as [number, Record<string, unknown>];
    expect(body.issues).toEqual([carriedIssue]);
    expect(body.parse_ms).toBe(1_000);
    expect(body.attachments_ms).toBe(2_000);
    expect(body.prepare_ms).toBe(3_000);
    expect(body.bytes_uploaded).toBe(10_500);
    expect(body.upload_ms as number).toBeGreaterThanOrEqual(40_000);
    expect(body.duration_ms as number).toBeGreaterThanOrEqual(60_000);
    expect(body.summary).toEqual(
      expect.objectContaining({
        files_total: 3,
        files_succeeded: 3,
        files_skipped: 0,
        messages_parsed: 30,
        messages_attempted: 30,
        messages_inserted: 30,
      }),
    );
    // The run ended, so its folder goes, the record with it.
    expect(invokeDeleteStagingMock).toHaveBeenCalled();
  });

  it("finishes a resumed Upload clean when the only rows are for conversations an earlier part sent", async () => {
    const alreadySent: ImportIssueEvent = {
      kind: "skip",
      step: "upload",
      item: "a.jsonl",
      reason: "already imported or skipped",
    };
    runMock.mockImplementation(
      runResultWithIssue(
        {
          summary: "Push finished.",
          report: okReport({
            conversations_total: 2,
            conversations_ok: 1,
            conversations_skipped: 1,
            results: [
              { file: "a.jsonl", status: "skipped", messages: 0, attachments: 0 },
              { file: "b.jsonl", status: "ok", messages: 10, attachments: 0 },
            ],
          }),
        },
        alreadySent,
      ),
    );
    const { result } = renderHook(() => useImportJob());
    await act(async () => {
      await result.current.startImport(baseForm, {
        sessionId: 99,
        stagingDir: "/home/u/message-crate/staging-260830",
      });
    });

    expect(result.current.summaryView?.status).toBe("completed");
    const [, body] = completeImportMock.mock.calls[0] as [number, Record<string, unknown>];
    expect(body.issues).toEqual([]);
  });

  it("still posts /complete against the resumed session id", async () => {
    const { result } = renderHook(() => useImportJob());

    await act(async () => {
      await result.current.startImport(baseForm, {
        sessionId: 99,
        stagingDir: "/home/u/message-crate/staging-260830",
      });
    });

    expect(result.current.phase).toBe("done");
    const completeCall = completeImportMock.mock.calls.find(([id]) => id === 99);
    expect(completeCall).toBeDefined();
  });
});

const validSnapshot = {
  source: "imessage-ios",
  backupPath: "/backups/iphone.tar",
  attachmentMedia: "copy",
  maxResolution: "720p",
  maxFps: "30",
  minSizeMb: "20",
  ownerPhones: ["+15551234567"],
  ownerEmails: [],
  obfuscate: false,
  isAndroidSms: false,
  attachmentRoot: "",
  appleContacts: "",
  whatsappWa: "",
  whatsappMedia: "",
  whatsappDb: "",
  whatsappBusiness: false,
  whatsappOwnerPhone: "",
  timeZone: "America/New_York",
  backupPasswordGiven: false,
  whatsappKeyGiven: false,
  assetMaxBytes: 512 * MIB,
};

describe("restoreFormFromSnapshot", () => {
  it("rebuilds form values from a stored snapshot, with the secrets left empty", () => {
    const { backupPasswordGiven: _password, whatsappKeyGiven: _key, ...settings } = validSnapshot;
    expect(restoreFormFromSnapshot(validSnapshot)).toEqual({
      ...settings,
      backupPassword: "",
      whatsappKey: "",
    });
  });

  it.each([
    ["a backup password", { backupPasswordGiven: true }, "backupPassword"],
    ["a WhatsApp key", { whatsappKeyGiven: true }, "whatsappKey"],
    ["neither", {}, null],
  ])("says which secret the stored Import Run was started with (%s)", (_label, given, secret) => {
    expect(snapshotSecret({ ...validSnapshot, ...given })).toBe(secret);
  });

  it("says no secret for a snapshot that is not an object", () => {
    expect(snapshotSecret(null)).toBeNull();
  });

  it.each([
    ["null", null],
    ["undefined", undefined],
    ["a string", "not an object"],
    ["an empty object", {}],
    ["a snapshot missing most fields", { source: "imessage-ios" }],
    ["an invalid attachmentMedia", { ...validSnapshot, attachmentMedia: "not-a-real-mode" }],
    ["a non-array ownerPhones", { ...validSnapshot, ownerPhones: "+15551234567" }],
    ["a non-boolean obfuscate", { ...validSnapshot, obfuscate: "yes" }],
    ["a non-boolean backupPasswordGiven", { ...validSnapshot, backupPasswordGiven: "yes" }],
    ["a snapshot with no whatsappKeyGiven", { ...validSnapshot, whatsappKeyGiven: undefined }],
    // Every stage measures against the limit the run was created under.
    ["a snapshot with no assetMaxBytes", { ...validSnapshot, assetMaxBytes: undefined }],
    ["a limit of zero", { ...validSnapshot, assetMaxBytes: 0 }],
  ])("returns null for a malformed snapshot (%s)", (_label, raw) => {
    expect(restoreFormFromSnapshot(raw)).toBeNull();
  });
});

function activeSession(overrides: Partial<ActiveImportSession> = {}): ActiveImportSession {
  return {
    id: 1,
    source: "imessage",
    mode: "append",
    status: "running",
    started_at: "2026-08-30T00:00:00Z",
    stage: "staging_review",
    staging_dir: "/home/u/message-crate/staging-260830",
    device_id: "this-device",
    form: validSnapshot,
    source_fingerprint: null,
    source_identities: null,
    summary: null,
    ...overrides,
  };
}

describe("useImportJob resumeAtReview", () => {
  beforeEach(() => {
    resetImportRun();
    runMock.mockReset();
    cancelMock.mockReset();
    createImportMock.mockReset();
    createImportMock.mockResolvedValue({ id: 1 });
    completeImportMock.mockReset();
    completeImportMock.mockResolvedValue({});
    resolveImportStagingDirMock.mockReset();
    invokePathStatMock.mockReset();
    invokeExtractMock.mockReset();
    invokePushMock.mockReset();
    readRunRecordMock.mockReset();
    saveRunRecordMock.mockReset();
    invokeSummarizeStagingMock.mockReset();
    invokeTranscodeStagingMock.mockReset();
    invokeDeleteStagingMock.mockReset();
    probeFfmpegToolsMock.mockReset();
    probeFfmpegToolsMock.mockResolvedValue(okProbe());
    setImportStageMock.mockReset();
    setImportStageMock.mockResolvedValue(undefined);
    discardImportSessionMock.mockReset();
    discardImportSessionMock.mockResolvedValue(undefined);
  });

  it("recomputes the summary fresh from the folder and lands on Gate 1 for a session waiting there", async () => {
    invokeSummarizeStagingMock.mockResolvedValueOnce(stagingSummary({ conversations: 9 }));
    const { result } = renderHook(() => useImportJob());

    await act(async () => {
      await result.current.resumeAtReview(activeSession({ stage: "staging_review" }), form());
    });

    expect(invokeSummarizeStagingMock).toHaveBeenCalledTimes(1);
    expect(result.current.phase).toBe("staging_review");
    expect(result.current.stagingSummary?.conversations).toBe(9);
    // Decision 39: landing on a gate to look at it again writes nothing.
    expect(setImportStageMock).not.toHaveBeenCalled();
    // Copy mode has no Media row: two rows, Staging already marked done,
    // matching the state a fresh startImport run would show right before
    // the Staging Review.
    expect(result.current.steps.map((s) => s.status)).toEqual(["done", "pending"]);
    expect(result.current.mediaPartiallyRan).toBe(false);
  });

  it("keeps the Staging issues of a run resumed at a Review, through to its completion", async () => {
    // Staging ran before the app closed; its issues were only in memory.
    const stagingIssue = {
      kind: "error",
      stage: "staging",
      item: "IMG_2.HEIC",
      reason: "could not be decrypted",
    };
    readRunRecordMock.mockResolvedValue({ issues: [stagingIssue], parseMs: 1_500 });
    invokeSummarizeStagingMock.mockResolvedValue(stagingSummary());
    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    const { result } = renderHook(() => useImportJob());

    await act(async () => {
      await result.current.resumeAtReview(activeSession({ stage: "staging_review" }), form());
    });
    await act(() => result.current.approve());

    const [, body] = completeImportMock.mock.calls[0] as [number, Record<string, unknown>];
    expect(body.issues).toEqual([stagingIssue]);
    expect(body.parse_ms).toBe(1_500);
    expect(result.current.summaryView?.status).toBe("completed_with_issues");
  });

  it("rebuilds a 3-row step list for a convert-mode session resuming at the Staging Review, Media pending", async () => {
    invokeSummarizeStagingMock.mockResolvedValueOnce(stagingSummary({ conversations: 9 }));
    const { result } = renderHook(() => useImportJob());

    await act(async () => {
      await result.current.resumeAtReview(
        activeSession({ stage: "staging_review" }),
        form({ attachmentMedia: "convert" }),
      );
    });

    expect(result.current.phase).toBe("staging_review");
    expect(result.current.steps.map((s) => s.label)).toEqual(["Staging", "Media", "Upload"]);
    expect(result.current.steps.map((s) => s.status)).toEqual(["done", "pending", "pending"]);
  });

  it("resumes at the Media Review showing the STORED plan for Staging and a RECOMPUTED summary for Media", async () => {
    const approved = stagingSummary({
      conversations: 3,
      attachments: 5,
    });
    const actual = stagingSummary({
      conversations: 3,
      attachments: 2,
    });
    invokeSummarizeStagingMock.mockResolvedValueOnce(actual);

    const { result } = renderHook(() => useImportJob());
    await act(async () => {
      await result.current.resumeAtReview(
        activeSession({ stage: "media_review", summary: approved }),
        form({ attachmentMedia: "convert" }),
      );
    });

    expect(invokeSummarizeStagingMock).toHaveBeenCalledTimes(1);
    expect(result.current.phase).toBe("media_review");
    // The Staging row shows what was approved before Media; what the
    // person is deciding on is always recomputed from the folder. The two
    // genuinely differ here, so a bug that fed one value to both shows.
    expect(result.current.stagingSummary).toEqual(approved);
    expect(result.current.mediaSummary).toEqual(actual);
    // Media's own report is gone on a resume: unknown, not zero.
    expect(result.current.mediaFailedCount).toBeNull();
    // Decision 39: landing on a gate to look at it again writes nothing.
    expect(setImportStageMock).not.toHaveBeenCalled();
    // The media pass already ran (in an earlier session) to get here -- its
    // row shows done, not pending, and there are 3 of them.
    expect(result.current.steps).toHaveLength(3);
    expect(result.current.steps[1]).toMatchObject({ label: "Media", status: "done" });
  });

  it("re-runs the media pass on a resume at transcode, then lands on Gate 2", async () => {
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );
    const approved = stagingSummary({ conversations: 7 });
    invokeSummarizeStagingMock.mockResolvedValueOnce(stagingSummary({ conversations: 7 }));

    const { result } = renderHook(() => useImportJob());
    await act(async () => {
      await result.current.resumeAtReview(
        activeSession({ stage: "media", summary: approved }),
        form({ attachmentMedia: "convert" }),
      );
    });

    expect(invokeTranscodeStagingMock).toHaveBeenCalled();
    expect(result.current.phase).toBe("media_review");
    // The stage write sequence matches the normal flow: "media" is
    // (idempotently) set again, then "media_review", both carrying the
    // plan stored at the last gate.
    expect(setImportStageMock).toHaveBeenCalledWith(1, "media", approved);
    expect(setImportStageMock).toHaveBeenCalledWith(1, "media_review", approved);
  });

  it("shows a 3-row list with the Media row active while the pass re-runs on a transcode resume", async () => {
    // A deliberately unresolved run() call, so the state mid-pass can be
    // inspected before the pass (and the resume) finishes -- the same
    // pattern the double-click guard test above uses.
    let resolveTranscode!: (value: TauriJobResult) => void;
    const pending = new Promise<TauriJobResult>((resolve) => {
      resolveTranscode = resolve;
    });
    runMock.mockImplementationOnce(async (fn: () => Promise<unknown>) => {
      await fn();
      return pending;
    });
    invokeSummarizeStagingMock.mockResolvedValueOnce(stagingSummary({ conversations: 7 }));

    const { result } = renderHook(() => useImportJob());
    let resumed!: Promise<void>;
    await act(async () => {
      resumed = result.current.resumeAtReview(
        activeSession({ stage: "media" }),
        form({ attachmentMedia: "convert" }),
      );
      // Let the microtasks up to (and including) invokeTranscodeStaging's
      // own call run, without waiting for `pending` to settle.
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(result.current.steps).toHaveLength(3);
    expect(result.current.steps.map((s) => s.status)).toEqual(["done", "active", "pending"]);

    await act(async () => {
      resolveTranscode({ summary: "Transcode finished.", transcode: undefined });
      await resumed;
    });
    expect(result.current.phase).toBe("media_review");
  });

  it("falls back to Gate 1 instead of running the pass when ffmpeg is missing on a transcode resume", async () => {
    probeFfmpegToolsMock.mockResolvedValue({
      ok: false,
      ffmpeg_path: null,
      ffprobe_path: null,
      error: "ffmpeg not found",
    });
    invokeSummarizeStagingMock.mockResolvedValueOnce(stagingSummary({ conversations: 7 }));

    const { result } = renderHook(() => useImportJob());
    await act(async () => {
      await result.current.resumeAtReview(
        activeSession({ stage: "media" }),
        form({ attachmentMedia: "convert" }),
      );
    });

    expect(invokeTranscodeStagingMock).not.toHaveBeenCalled();
    expect(result.current.phase).toBe("staging_review");
    expect(result.current.mediaToolsMissing).toBe(true);
    // The folder may hold a mix of originals and already-converted files --
    // Gate 1's "has not run yet" copy would be wrong here.
    expect(result.current.mediaPartiallyRan).toBe(true);
    expect(result.current.steps.map((s) => s.status)).toEqual(["done", "pending", "pending"]);
  });

  it("a malformed stored summary does not block a resume — it proceeds with no approved plan", async () => {
    const actual = stagingSummary({
      conversations: 4,
    });
    actual.forecasts = [
      {
        path: "attachments/x.mov",
        name: "x.mov",
        sizeBytes: 1,
        estimateBytes: 1,
        verdict: "likely_fits",
      },
    ];
    invokeSummarizeStagingMock.mockResolvedValueOnce(actual);

    const { result } = renderHook(() => useImportJob());
    await act(async () => {
      await result.current.resumeAtReview(
        activeSession({ stage: "media_review", summary: "not a valid staging summary" }),
        form({ attachmentMedia: "convert" }),
      );
    });

    expect(result.current.phase).toBe("media_review");
    // No stored plan to show for Staging: the resume still lands on the
    // review with the recomputed folder, instead of blocking or throwing.
    expect(result.current.stagingSummary).toBeNull();
    expect(result.current.mediaSummary).toEqual(actual);
  });

  it("does nothing for a session at a stage this function doesn't handle", async () => {
    const { result } = renderHook(() => useImportJob());
    await act(async () => {
      await result.current.resumeAtReview(activeSession({ stage: "upload" }), form());
    });

    expect(invokeSummarizeStagingMock).not.toHaveBeenCalled();
    expect(result.current.phase).toBe("form");
  });

  it.each(["staging_review", "media_review"] as const)(
    "a recompute failure at %s does not complete the session or write a stage — it retries from the form",
    async (stage) => {
      invokeSummarizeStagingMock.mockRejectedValueOnce(new Error("disk unavailable"));

      const { result } = renderHook(() => useImportJob());
      await act(async () => {
        await result.current.resumeAtReview(
          activeSession({ stage, summary: stagingSummary() }),
          form({ attachmentMedia: "convert" }),
        );
      });

      // Decision 37: only an explicit discard ends a waiting session. A
      // transient read failure must not complete it (freeing the slot) or
      // move it to a stage the folder never actually reached.
      expect(completeImportMock).not.toHaveBeenCalled();
      expect(setImportStageMock).not.toHaveBeenCalled();
      expect(result.current.phase).toBe("form");
      expect(result.current.resumeError).toContain("disk unavailable");
    },
  );

  it("clears a stale resumeError once a later resume attempt starts", async () => {
    invokeSummarizeStagingMock.mockRejectedValueOnce(new Error("disk unavailable"));
    const { result } = renderHook(() => useImportJob());
    await act(async () => {
      await result.current.resumeAtReview(
        activeSession({ stage: "staging_review" }),
        form({ attachmentMedia: "convert" }),
      );
    });
    expect(result.current.resumeError).not.toBeNull();

    invokeSummarizeStagingMock.mockResolvedValueOnce(stagingSummary());
    await act(async () => {
      await result.current.resumeAtReview(
        activeSession({ stage: "staging_review" }),
        form({ attachmentMedia: "convert" }),
      );
    });
    expect(result.current.resumeError).toBeNull();
    expect(result.current.phase).toBe("staging_review");
  });
});

describe("parseStoredStagingSummary", () => {
  it("round-trips a valid stored summary", () => {
    const valid = stagingSummary({ conversations: 3, attachments: 2 });
    valid.forecasts = [
      {
        path: "attachments/x.mov",
        name: "x.mov",
        sizeBytes: 10,
        estimateBytes: 8,
        verdict: "likely_fits",
      },
    ];
    expect(parseStoredStagingSummary(valid)).toEqual(valid);
  });

  it("returns undefined for null, non-objects, and an empty object", () => {
    expect(parseStoredStagingSummary(null)).toBeUndefined();
    expect(parseStoredStagingSummary(undefined)).toBeUndefined();
    expect(parseStoredStagingSummary("not a summary")).toBeUndefined();
    expect(parseStoredStagingSummary({})).toBeUndefined();
  });

  it("returns undefined when a required field is missing", () => {
    const valid = stagingSummary();
    const { attachmentBytes: _attachmentBytes, ...missingAttachmentBytes } = valid;
    expect(parseStoredStagingSummary(missingAttachmentBytes)).toBeUndefined();
  });

  it("returns undefined when a forecasts row is malformed", () => {
    const missingVerdict = {
      ...stagingSummary(),
      forecasts: [{ path: "attachments/x.mov", name: "x.mov", sizeBytes: 1, estimateBytes: 1 }],
    };
    expect(parseStoredStagingSummary(missingVerdict)).toBeUndefined();

    const badVerdict = {
      ...stagingSummary(),
      forecasts: [
        {
          path: "attachments/x.mov",
          name: "x.mov",
          sizeBytes: 1,
          estimateBytes: 1,
          verdict: "not_a_real_verdict",
        },
      ],
    };
    expect(parseStoredStagingSummary(badVerdict)).toBeUndefined();
  });
});

describe("one desktop app, two accounts (#1085)", () => {
  const ACCOUNT_A = { token: "token-of-A", accountId: 1 };
  const ACCOUNT_B = { token: "token-of-B", accountId: 2 };

  beforeEach(() => {
    auth = ACCOUNT_A;
    resetImportRun();
    runMock.mockReset();
    runMock.mockImplementationOnce(runResult(EXTRACT_RESULT));
    cancelMock.mockReset();
    getServerStateMock.mockResolvedValue({ asset_max_bytes: 512 * MIB });
    createImportMock.mockReset();
    createImportMock.mockResolvedValue({ id: 1 });
    completeImportMock.mockReset();
    completeImportMock.mockResolvedValue({});
    resolveImportStagingDirMock.mockReset();
    resolveImportStagingDirMock.mockResolvedValue("/home/sam/message-crate/staging-iphone");
    invokePathStatMock.mockResolvedValue(null);
    invokeSummarizeStagingMock.mockResolvedValue(stagingSummary());
    invokeDeleteStagingMock.mockReset();
    invokePushMock.mockReset();
    readRunRecordMock.mockReset();
    saveRunRecordMock.mockReset();
    probeFfmpegToolsMock.mockResolvedValue(okProbe());
    setImportStageMock.mockReset();
    setImportStageMock.mockResolvedValue(undefined);
    discardImportSessionMock.mockReset();
    discardImportSessionMock.mockResolvedValue(undefined);
    invokeImessageBackupIdentitiesMock.mockResolvedValue([]);
    loadAccountProfileMock.mockResolvedValue({ phones: [], emails: [] });
  });

  afterEach(() => {
    auth = { token: "test-token", accountId: 1 };
  });

  it("does not offer account A's parked form to account B", async () => {
    invokeImessageBackupIdentitiesMock.mockResolvedValue(["+15550001111"]);
    loadAccountProfileMock.mockResolvedValue({ phones: ["+15550109999"], emails: [] });
    const a = renderHook(() => useImportJob());
    await act(() => a.result.current.startImport({ ...form(), backupPassword: "secret-of-A" }));
    expect(a.result.current.phase).toBe("identity_stop");
    a.unmount();

    // A logs out, B logs in on the same desktop app and opens Import.
    auth = ACCOUNT_B;
    const b = renderHook(() => useImportJob());
    expect(b.result.current.phase).toBe("form");
    expect(b.result.current.sourceIdentities).toBeNull();
    await act(() => b.result.current.continueAfterIdentityStop());
    expect(createImportMock).not.toHaveBeenCalled();
  });

  it("creates no run when account A logs out while its backup is being read", async () => {
    let finishProbe: ((identities: string[]) => void) | null = null;
    invokeImessageBackupIdentitiesMock.mockImplementation(
      () =>
        new Promise<string[]>((resolve) => {
          finishProbe = resolve;
        }),
    );
    const a = renderHook(() => useImportJob());
    let runOfA: Promise<void> = Promise.resolve();
    act(() => {
      runOfA = a.result.current.startImport({ ...form(), backupPassword: "secret-of-A" });
    });
    await waitFor(() => expect(finishProbe).not.toBeNull());

    // A logs out and B logs in before the probe of A's backup ends. The
    // server calls go with B's session from here on.
    auth = ACCOUNT_B;
    await act(async () => {
      finishProbe?.([]);
      await runOfA;
    });

    expect(createImportMock).not.toHaveBeenCalled();
    expect(runMock).not.toHaveBeenCalled();
  });

  it("shows account A's waiting review to A only, and B's actions leave it alone", async () => {
    const a = renderHook(() => useImportJob());
    await act(() => a.result.current.startImport(form()));
    expect(a.result.current.phase).toBe("staging_review");
    a.unmount();

    auth = ACCOUNT_B;
    const b = renderHook(() => useImportJob());
    expect(b.result.current.phase).toBe("form");
    expect(b.result.current.form).toBeNull();
    expect(b.result.current.stagingDir).toBeNull();
    expect(b.result.current.stagingSummary).toBeNull();

    await act(() => b.result.current.cancelRun());
    await act(() => b.result.current.approve());
    act(() => b.result.current.returnToForm());
    // A's staged folder and A's run are A's to delete or carry on.
    expect(invokeDeleteStagingMock).not.toHaveBeenCalled();
    expect(discardImportSessionMock).not.toHaveBeenCalled();
    expect(invokePushMock).not.toHaveBeenCalled();
    b.unmount();

    // A logs back in and finds the run where it was left.
    auth = ACCOUNT_A;
    const aAgain = renderHook(() => useImportJob());
    expect(aAgain.result.current.phase).toBe("staging_review");
    expect(aAgain.result.current.stagingDir).toBe("/home/sam/message-crate/staging-iphone");
  });

  it("stops account A's Staging before account B's run starts, and keeps A's end out of it", async () => {
    resolveImportStagingDirMock
      .mockResolvedValueOnce("/home/sam/message-crate/staging-of-A")
      .mockResolvedValueOnce("/home/sam/message-crate/staging-of-B");
    createImportMock.mockResolvedValueOnce({ id: 11 }).mockResolvedValueOnce({ id: 22 });
    // A's extract runs until a Cancel stops it, the way the desktop job does.
    let stopExtractOfA: (() => void) | null = null;
    runMock.mockReset();
    runMock.mockImplementationOnce(async (fn) => {
      await fn();
      await new Promise<void>((resolve) => {
        stopExtractOfA = resolve;
      });
      throw new Error("cancelled");
    });
    runMock.mockImplementationOnce(runResult(EXTRACT_RESULT));
    cancelMock.mockImplementation(async () => stopExtractOfA?.());

    const a = renderHook(() => useImportJob());
    let runOfA: Promise<void> = Promise.resolve();
    act(() => {
      runOfA = a.result.current.startImport(form());
    });
    await waitFor(() => expect(stopExtractOfA).not.toBeNull());
    a.unmount();

    auth = ACCOUNT_B;
    const b = renderHook(() => useImportJob());
    await act(() => b.result.current.startImport(form()));
    await act(() => runOfA);

    expect(cancelMock).toHaveBeenCalled();
    // A's run stays open on the server, resumable from its own folder.
    expect(completeImportMock).not.toHaveBeenCalled();
    expect(saveRunRecordMock).toHaveBeenCalledWith(
      expect.objectContaining({ staging_dir: "/home/sam/message-crate/staging-of-A" }),
    );
    // B's run is B's alone.
    expect(b.result.current.phase).toBe("staging_review");
    expect(b.result.current.importSessionId).toBe(22);
    expect(b.result.current.stagingDir).toBe("/home/sam/message-crate/staging-of-B");
    expect(importRunStore.get().accountId).toBe(ACCOUNT_B.accountId);
  });
});

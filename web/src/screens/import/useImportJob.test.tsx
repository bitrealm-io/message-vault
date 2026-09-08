/** @vitest-environment jsdom */

// Pins the wiring the 2026-08-27 incident broke: useImportJob must pass
// pushResult.report into importOutcome, use that outcome as
// finalSummary.status, and send it as `status` in the /complete POST body.
// The verdict logic itself is covered exhaustively by importOutcome.test.ts;
// nothing there would have caught a revert of the three lines that connect
// that logic to the hook, because those tests call importOutcome directly.
//
// It also pins the two-gate flow added afterward: startImport now stops at
// Gate 1 instead of pushing straight through, approveGate runs the media
// pass (when there is one) and stops at Gate 2, and declineGate closes the
// session and deletes the staging folder. Every push assertion below goes
// through approveGate first, because there is no other way to reach it.

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ActiveImportSession } from "../../lib/importSession";
import type {
  FfmpegToolsProbe,
  PushFinishedReport,
  StagingSummary,
  TauriJobResult,
} from "../../lib/tauri";
import type { AttachmentMediaMode, ImportIssueEvent, ImportProgressEvent } from "../../lib/types";
import { restoreFormFromSnapshot } from "./formSnapshot";
import { gateDelta } from "./gateDelta";

const createImportMock = vi.fn();
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
  invokeExtract: (...args: unknown[]) => invokeExtractMock(...args),
  invokePush: (...args: unknown[]) => invokePushMock(...args),
  invokePathStat: (...args: unknown[]) => invokePathStatMock(...args),
  invokeSummarizeStaging: (...args: unknown[]) => invokeSummarizeStagingMock(...args),
  invokeTranscodeStaging: (...args: unknown[]) => invokeTranscodeStagingMock(...args),
  invokeDeleteStaging: (...args: unknown[]) => invokeDeleteStagingMock(...args),
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

vi.mock("../../hooks/useTauriJob", () => ({
  useTauriJob: () => ({
    run: runMock,
    cancel: cancelMock,
    running: false,
    finished: true,
    log: [],
  }),
}));

vi.mock("../../lib/api", () => ({
  getBaseUrl: () => "http://127.0.0.1:8080",
}));

// The two vault calls this hook makes. Everything else in vaultApi stays real,
// since other modules in this graph import from it.
vi.mock("../../lib/vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/vaultApi")>()),
  createImport: (...args: unknown[]) => createImportMock(...args),
  completeImport: (...args: unknown[]) => completeImportMock(...args),
}));

vi.mock("../../lib/auth", () => ({
  useAuth: () => ({ token: "test-token" }),
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
const { useImportJob, parseStoredStagingSummary } = await import("./useImportJob");

/**
 * `runMock` stands in for `useTauriJob().run`, which always calls the
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
 * Like `runResult`, but also fires `onIssue` — the real `useTauriJob` does
 * this from the job's own event stream, which the mock above otherwise never
 * exercises. Needed to simulate a push that reports a skip.
 */
function runResultWithIssue(result: TauriJobResult, issue: ImportIssueEvent) {
  return async (
    fn: () => Promise<unknown>,
    options?: { onIssue?: (event: ImportIssueEvent) => void },
  ) => {
    await fn();
    options?.onIssue?.(issue);
    return result;
  };
}

function failedReport(): PushFinishedReport {
  return {
    ok: false,
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
    results: [],
  };
}

function okReport(overrides: Partial<PushFinishedReport> = {}): PushFinishedReport {
  return {
    ok: true,
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
    results: [],
    ...overrides,
  };
}

function stagingSummary(overrides: Partial<StagingSummary> = {}): StagingSummary {
  return {
    conversations: 1,
    messages: 10,
    contactIdentifiers: [],
    attachments: 0,
    attachmentBytes: 0,
    verdictCounts: {
      fitsAsIs: 0,
      likelyFits: 0,
      mayGrow: 0,
      probablyTooBig: 0,
      cannotProcess: 0,
    },
    forecasts: [],
    ...overrides,
  };
}

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
  force: false,
  obfuscate: false,
  isAndroidSms: false,
  attachmentRoot: "",
  appleContacts: "",
  whatsappKey: "",
  whatsappWa: "",
  whatsappMedia: "",
  whatsappDb: "",
  whatsappBusiness: false,
};

function form(overrides: { attachmentMedia?: AttachmentMediaMode } = {}) {
  return { ...baseForm, ...overrides };
}

describe("useImportJob wiring", () => {
  beforeEach(() => {
    runMock.mockReset();
    // Every test's first run() call is extract; a test that also calls
    // approveGate() chains a second implementation for the media pass or
    // the push on top of this one.
    runMock.mockImplementationOnce(runResult(EXTRACT_RESULT));
    cancelMock.mockReset();
    createImportMock.mockReset();
    createImportMock.mockResolvedValue({ id: 1 });
    completeImportMock.mockReset();
    completeImportMock.mockResolvedValue({});
    resolveImportStagingDirMock.mockReset();
    resolveImportStagingDirMock.mockResolvedValue("/home/sam/message-vault/staging-iphone");
    invokePathStatMock.mockReset();
    invokePathStatMock.mockResolvedValue(null);
    invokeExtractMock.mockReset();
    invokePushMock.mockReset();
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

  it("stops at the first gate instead of uploading", async () => {
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    expect(result.current.phase).toBe("gate_1");
    expect(invokePushMock).not.toHaveBeenCalled();
    expect(invokeTranscodeStagingMock).not.toHaveBeenCalled();
  });

  it("asks the exporter to stage originals under convert", async () => {
    // The desktop runs the media pass itself, after the gate.
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    expect(invokeExtractMock).toHaveBeenCalledWith(
      expect.objectContaining({ attachment_media: "copy" }),
    );
  });

  it("records the stage as it goes", async () => {
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    // No plan exists yet at either call — `setImportStage` still receives a
    // (harmlessly `undefined`) third argument; see `moveStage`.
    expect(setImportStageMock).toHaveBeenCalledWith(1, "write", undefined);
    expect(setImportStageMock).toHaveBeenCalledWith(1, "awaiting_gate_1", undefined);
  });

  it("runs the media pass then stops at the second gate", async () => {
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approveGate());
    expect(invokeTranscodeStagingMock).toHaveBeenCalled();
    expect(result.current.phase).toBe("gate_2");
    expect(invokePushMock).not.toHaveBeenCalled();
  });

  it("routes a progress event arriving during summarize to the staging row", async () => {
    // W6: `summarize_staging` (Rust) emits `extract:progress` with
    // `step: "prepare"` while it walks a big folder, but nothing used to
    // subscribe, so those events had nowhere to go and a huge folder's gate
    // looked frozen. The mocked `invokeSummarizeStaging` fires one here,
    // mid-call, through the callbacks `onExtractEvents` was given — exactly
    // what the real Tauri event stream would do.
    invokeSummarizeStagingMock.mockReset();
    invokeSummarizeStagingMock.mockImplementationOnce(async () => {
      lastExtractEventCallbacks?.onProgress?.({ step: "prepare", done: 50, total: 200 });
      return stagingSummary();
    });
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));

    expect(result.current.phase).toBe("gate_1");
    // "Copy to staging" is row 1 in every mode (attachments/prepare share it).
    expect(result.current.steps[1]?.detail).toBe("Preparing 50/200");
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
        options?: { onProgress?: (event: ImportProgressEvent) => void },
      ) => {
        await fn();
        options?.onProgress?.({
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
    expect(result.current.phase).toBe("gate_1");
  });

  it("uploads straight from the first gate under copy, because there is no second one", async () => {
    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    await act(() => result.current.approveGate());
    expect(invokeTranscodeStagingMock).not.toHaveBeenCalled();
    expect(invokePushMock).toHaveBeenCalled();
  });

  it("carries the plan approved at Gate 1 into the pushing stage call under copy", async () => {
    runMock.mockImplementationOnce(runResult({ summary: "Push finished.", report: okReport() }));
    const approved = stagingSummary({ conversations: 3 });
    invokeSummarizeStagingMock.mockResolvedValueOnce(approved);
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "copy" })));
    await act(() => result.current.approveGate());

    expect(setImportStageMock).toHaveBeenCalledWith(1, "pushing", approved);
  });

  it("recomputes the summary after the media pass rather than adjusting the old one", async () => {
    // Decision 39: the folder is the truth.
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    invokeSummarizeStagingMock.mockClear();
    await act(() => result.current.approveGate());
    expect(invokeSummarizeStagingMock).toHaveBeenCalledTimes(1);
  });

  it("writes transcode carrying the Gate-1 plan, then awaiting_gate_2 carrying it too", async () => {
    // Important 4: a crash mid-pass must not leave summary_json null with
    // no baseline for a later resume — so the plan rides the "transcode"
    // stage call too, not only the one after it.
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );
    const approved = stagingSummary({ conversations: 5 });
    invokeSummarizeStagingMock.mockResolvedValueOnce(approved);
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approveGate());

    expect(setImportStageMock).toHaveBeenCalledWith(1, "transcode", approved);
    expect(setImportStageMock).toHaveBeenCalledWith(1, "awaiting_gate_2", approved);
  });

  it("declining closes the session and deletes the folder", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/staging/run-1");
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.declineGate());
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
    await act(() => result.current.declineGate());
    expect(invokeDeleteStagingMock).toHaveBeenCalled();
  });

  it("still discards the session, and still returns to the form, even when deleting the folder fails", async () => {
    // The other direction of the same guarantee: a regression to sequential
    // discard-then-delete (each awaited without independent handling) would
    // let a rejected delete propagate out of declineGate and skip
    // returnToForm — leaving the screen stuck on Gate 1 with a session the
    // vault already considers discarded.
    invokeDeleteStagingMock.mockRejectedValueOnce(new Error("disk full"));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.declineGate());
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
    await act(() => result.current.approveGate());
    expect(result.current.phase).toBe("gate_2");

    await act(() => result.current.declineGate());

    expect(discardImportSessionMock).toHaveBeenCalledWith(1);
    expect(invokeDeleteStagingMock).toHaveBeenCalledWith({ staging_dir: "/staging/run-2" });
    expect(result.current.phase).toBe("form");
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
    await act(() => result.current.approveGate()); // Gate 1 -> media pass -> Gate 2
    expect(result.current.phase).toBe("gate_2");

    await act(() => result.current.approveGate()); // Gate 2 -> pushing

    expect(setImportStageMock).toHaveBeenCalledWith(1, "pushing", recomputed);
    expect(setImportStageMock).not.toHaveBeenCalledWith(1, "pushing", gate1Approved);
  });

  it("reaches a failed push through Gate 2 the same way copy mode does through Gate 1", async () => {
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );
    runMock.mockImplementationOnce(
      runResult({ summary: "Push finished.", report: failedReport() }),
    );
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approveGate());
    expect(result.current.phase).toBe("gate_2");

    await act(() => result.current.approveGate());

    expect(result.current.phase).toBe("done");
    expect(result.current.summaryView?.status).toBe("failed");
  });

  it("unwedges on a cancelled media pass instead of freezing the screen", async () => {
    // Critical: transcode_staging used to end a cancelled pass quietly (an
    // extract:log line, Ok(())) with no extract:finished and no
    // extract:error, so awaitTauriJob's promise never settled and the
    // screen was stuck. It now reports through extract:error like any other
    // failure, so run() rejects here exactly as it would for a real error.
    runMock.mockImplementationOnce(async (fn: () => Promise<unknown>) => {
      await fn();
      throw new Error("canceled");
    });
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approveGate());

    expect(invokePushMock).not.toHaveBeenCalled();
    expect(result.current.phase).toBe("done");
    expect(result.current.summaryView?.status).toBe("canceled");
    // The session stays wherever the run actually got to — "transcode" —
    // never advanced to a stage the cancelled run never reached.
    expect(setImportStageMock).not.toHaveBeenCalledWith(1, "awaiting_gate_2", expect.anything());
    expect(setImportStageMock).not.toHaveBeenCalledWith(1, "pushing", expect.anything());
  });

  it("does not complete the session on a cancelled media pass, so it stays resumable", async () => {
    // Decision 36 routes a cancellation mid-transcode to the same recovery
    // as a crash at that stage; decision 37 says only an explicit discard
    // ends a waiting session. Posting /complete would free the one-active-
    // session slot and drop the session out of GET /v1/imports?status=running,
    // stranding the staged folder with no session left to resume it
    // through — even though the "canceled" outcome is still shown locally.
    runMock.mockImplementationOnce(async (fn: () => Promise<unknown>) => {
      await fn();
      throw new Error("canceled");
    });
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approveGate());

    expect(result.current.summaryView?.status).toBe("canceled");
    expect(completeImportMock.mock.calls.some(([id]) => id === 1)).toBe(false);
  });

  it("still completes the session as failed when the media pass genuinely fails", async () => {
    // Unlike a cancellation, a broken ffmpeg (or any other real failure)
    // must not lock the account out of importing — the run still completes
    // and frees the slot, same as before.
    runMock.mockRejectedValueOnce(new Error("ffmpeg exited with status 1"));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approveGate());

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

    expect(result.current.summaryView?.status).toBe("canceled");
    expect(completeImportMock.mock.calls.some(([id]) => id === 1)).toBe(false);
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
    // resume check re-finds the same session (stage awaiting_gate_1) and
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
    expect(setImportStageMock).toHaveBeenCalledWith(1, "awaiting_gate_1", undefined);
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
    // failure path: see the "does not strand the folder" test below).
    invokeSummarizeStagingMock.mockResolvedValueOnce(stagingSummary());
    invokeSummarizeStagingMock.mockRejectedValueOnce(new Error("disk full"));
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(form({ attachmentMedia: "convert" })));
    await act(() => result.current.approveGate());

    expect(invokePushMock).not.toHaveBeenCalled();
    expect(result.current.phase).toBe("done");
    expect(result.current.summaryView?.status).toBe("failed");
    expect(setImportStageMock).not.toHaveBeenCalledWith(1, "awaiting_gate_2", expect.anything());
  });

  it("does not run the media pass twice on a double click", async () => {
    let resolveTranscode!: (value: TauriJobResult) => void;
    const pending = new Promise<TauriJobResult>((resolve) => {
      resolveTranscode = resolve;
    });
    // Deliberately left unresolved: lets the two approveGate() calls below
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
      void result.current.approveGate();
      void result.current.approveGate();
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
    await act(() => result.current.approveGate());
    expect(invokePushMock).not.toHaveBeenCalled();
    expect(result.current.phase).toBe("done");
    expect(result.current.summaryView?.status).toBe("failed");
  });

  it("carries a report where every conversation failed through to a failed summary and /complete body", async () => {
    runMock.mockImplementationOnce(
      runResult({ summary: "Push finished.", report: failedReport() }),
    );
    const { result } = renderHook(() => useImportJob());

    await act(() => result.current.startImport(baseForm));
    await act(() => result.current.approveGate());

    // The wiring under test: the hook's own verdict, not importOutcome's.
    expect(result.current.summaryView?.status).toBe("failed");
    expect(result.current.phase).toBe("done");

    const completeCall = completeImportMock.mock.calls.find(([id]) => id === 1);
    expect(completeCall).toBeDefined();
    const [, body] = completeCall as [string, Record<string, unknown>];
    expect(body.status).toBe("failed");
    expect(body.ok).toBe(false);
  });

  it("records the staging folder and device on the session it creates", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/home/u/message-vault/staging-260830");
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
    expect(body.staging_dir).toBe("/home/u/message-vault/staging-260830");
    expect(body.form).toMatchObject({ source: "imessage-ios" });
  });

  it("keeps the backup password out of the stored form snapshot", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/tmp/staging");
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport({ ...baseForm, backupPassword: "hunter2" }));

    const body = createImportMock.mock.calls[0]?.[0] as Record<string, unknown>;
    expect(JSON.stringify(body.form)).not.toContain("hunter2");
  });

  it("moves the session to pushing before the upload starts", async () => {
    resolveImportStagingDirMock.mockResolvedValue("/tmp/staging");
    runMock.mockImplementationOnce(
      runResult({ summary: "Push finished.", report: failedReport() }),
    );

    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport(baseForm));
    await act(() => result.current.approveGate());

    const stageCall = setImportStageMock.mock.calls.find(([, stage]) => stage === "pushing");
    expect(stageCall).toBeDefined();
    expect(stageCall?.[0]).toBe(1);
  });

  it("assembles a 4-row step list in convert mode, stopping at the gate with the media row still pending", async () => {
    // Pins the mode-dependent assembly stepsFor/stepIndexFor exist for: this
    // hook does not run the media pass until Gate 1 is approved, so the row
    // must sit pending, not silently vanish or get marked done.
    resolveImportStagingDirMock.mockResolvedValue("/tmp/staging");

    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport({ ...baseForm, attachmentMedia: "convert" }));

    expect(result.current.phase).toBe("gate_1");
    expect(result.current.steps.map((s) => s.label)).toEqual([
      "Read backup",
      "Copy to staging",
      "Convert media",
      "Upload to vault",
    ]);
    expect(result.current.steps[2]?.status).toBe("pending");
    expect(result.current.steps[3]?.status).toBe("pending");
  });

  it("continues the convert-mode step list through the media pass into Gate 2", async () => {
    // Task 7 pinned the media row sitting pending after extract; this
    // continues the same run through approval: active while the pass runs,
    // done once it finishes.
    resolveImportStagingDirMock.mockResolvedValue("/tmp/staging");
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );

    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport({ ...baseForm, attachmentMedia: "convert" }));
    await act(() => result.current.approveGate());

    expect(result.current.phase).toBe("gate_2");
    expect(result.current.steps[2]?.status).toBe("done");
  });

  it("says the staging row was Copied under convert, since extract only stages originals now", async () => {
    // Important 5: extract stages originals under convert/compress too
    // (ruling 3) — the staging row must say what extract actually did, not
    // what the user ultimately asked for. The media row (index 2) still
    // tells the convert/compress story once the pass itself runs.
    resolveImportStagingDirMock.mockResolvedValue("/tmp/staging");
    const { result } = renderHook(() => useImportJob());
    await act(() => result.current.startImport({ ...baseForm, attachmentMedia: "convert" }));

    expect(result.current.steps[1]?.detail).toMatch(/^Copied /);
    expect(result.current.steps[1]?.detail).not.toMatch(/^Converted /);
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
          stagingDir: "/home/u/message-vault/staging-260830",
          identities: ["+15550001111"],
        });
      });
      expect(invokeImessageBackupIdentitiesMock).not.toHaveBeenCalled();
      expect(result.current.phase).toBe("gate_1");
      expect(result.current.sourceIdentities).toEqual(["+15550001111"]);
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
    runMock.mockReset();
    // A resumed run only ever calls run() once, for the push — and, like
    // the wiring tests above, must actually call the invoke function so
    // invokePush's args can be inspected.
    runMock.mockImplementation(runResult({ summary: "Push finished.", report: failedReport() }));
    cancelMock.mockReset();
    createImportMock.mockReset();
    createImportMock.mockResolvedValue({ id: 1 });
    completeImportMock.mockReset();
    completeImportMock.mockResolvedValue({});
    resolveImportStagingDirMock.mockReset();
    invokePathStatMock.mockReset();
    invokePathStatMock.mockResolvedValue(null);
    invokePushMock.mockReset();
    setImportStageMock.mockReset();
    setImportStageMock.mockResolvedValue(undefined);
    discardImportSessionMock.mockReset();
  });

  it("passes the resumed session id and staging dir through to invokePush", async () => {
    const { result } = renderHook(() => useImportJob());
    await act(async () => {
      await result.current.startImport(baseForm, {
        sessionId: 99,
        stagingDir: "/home/u/message-vault/staging-260830",
      });
    });

    expect(invokePushMock).toHaveBeenCalledTimes(1);
    expect(invokePushMock).toHaveBeenCalledWith(
      expect.objectContaining({
        import_id: 99,
        input_dir: "/home/u/message-vault/staging-260830",
      }),
    );
  });

  it("skips staging resolve, session create, and extract when resuming a push", async () => {
    const { result } = renderHook(() => useImportJob());

    await act(async () => {
      await result.current.startImport(baseForm, {
        sessionId: 99,
        stagingDir: "/home/u/message-vault/staging-260830",
      });
    });

    expect(resolveImportStagingDirMock).not.toHaveBeenCalled();
    expect(invokePathStatMock).not.toHaveBeenCalled();
    expect(createImportMock.mock.calls.length > 0).toBe(false);
    expect(runMock).toHaveBeenCalledTimes(1); // push only, no extract
    expect(result.current.stagingDir).toBe("/home/u/message-vault/staging-260830");
    expect(result.current.importSessionId).toBe(99);
  });

  it("marks the staging steps already staged and moves the session to pushing without a plan", async () => {
    // baseForm uses attachmentMedia "copy", which has no media step: Read
    // backup, Copy to staging, Upload to vault — three rows, not four.
    // Nothing was ever gated on a resumed run, so there is no approved plan
    // to carry — `setImportStage` still receives a third argument, but it's
    // `undefined`, which reaches the server identically to omitting it
    // entirely (JSON.stringify drops it).
    const { result } = renderHook(() => useImportJob());

    await act(async () => {
      await result.current.startImport(baseForm, {
        sessionId: 99,
        stagingDir: "/home/u/message-vault/staging-260830",
      });
    });

    expect(setImportStageMock).toHaveBeenCalledWith(99, "pushing", undefined);

    expect(result.current.steps).toHaveLength(3);
    for (const step of result.current.steps.slice(0, 2)) {
      expect(step.status).toBe("done");
      expect(step.detail).toBe("Already staged");
      expect(step.durationMs).toBeUndefined();
    }
    expect(result.current.steps[2]).toMatchObject({ label: "Upload to vault" });
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
        stagingDir: "/home/u/message-vault/staging-260830",
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
        stagingDir: "/home/u/message-vault/staging-260830",
      });
    });

    expect(result.current.summaryView?.status).toBe("completed_with_issues");
  });

  it("still posts /complete against the resumed session id", async () => {
    const { result } = renderHook(() => useImportJob());

    await act(async () => {
      await result.current.startImport(baseForm, {
        sessionId: 99,
        stagingDir: "/home/u/message-vault/staging-260830",
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
  force: false,
  obfuscate: false,
  isAndroidSms: false,
  attachmentRoot: "",
  appleContacts: "",
  whatsappWa: "",
  whatsappMedia: "",
  whatsappDb: "",
  whatsappBusiness: false,
};

describe("restoreFormFromSnapshot", () => {
  it("rebuilds form values from a stored snapshot, defaulting the omitted secrets", () => {
    expect(restoreFormFromSnapshot(validSnapshot)).toEqual({
      ...validSnapshot,
      backupPassword: "",
      whatsappKey: "",
    });
  });

  it.each([
    ["null", null],
    ["undefined", undefined],
    ["a string", "not an object"],
    ["an empty object", {}],
    ["a snapshot missing most fields", { source: "imessage-ios" }],
    ["an invalid attachmentMedia", { ...validSnapshot, attachmentMedia: "not-a-real-mode" }],
    ["a non-array ownerPhones", { ...validSnapshot, ownerPhones: "+15551234567" }],
    ["a non-boolean force", { ...validSnapshot, force: "yes" }],
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
    stage: "awaiting_gate_1",
    staging_dir: "/home/u/message-vault/staging-260830",
    device_id: "this-device",
    form: validSnapshot,
    source_fingerprint: null,
    source_identities: null,
    summary: null,
    ...overrides,
  };
}

describe("useImportJob resumeAtGate", () => {
  beforeEach(() => {
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
      await result.current.resumeAtGate(activeSession({ stage: "awaiting_gate_1" }), form());
    });

    expect(invokeSummarizeStagingMock).toHaveBeenCalledTimes(1);
    expect(result.current.phase).toBe("gate_1");
    expect(result.current.gateSummary?.conversations).toBe(9);
    // Decision 39: landing on a gate to look at it again writes nothing.
    expect(setImportStageMock).not.toHaveBeenCalled();
    // Copy mode has no media row -- three rows, the staged ones already
    // marked done, matching the state a fresh startImport run would show
    // right before Gate 1.
    expect(result.current.steps.map((s) => s.status)).toEqual(["done", "done", "pending"]);
    expect(result.current.mediaPartiallyRan).toBe(false);
  });

  it("rebuilds a 4-row step list for a convert-mode session resuming at Gate 1, media row pending", async () => {
    invokeSummarizeStagingMock.mockResolvedValueOnce(stagingSummary({ conversations: 9 }));
    const { result } = renderHook(() => useImportJob());

    await act(async () => {
      await result.current.resumeAtGate(
        activeSession({ stage: "awaiting_gate_1" }),
        form({ attachmentMedia: "convert" }),
      );
    });

    expect(result.current.phase).toBe("gate_1");
    expect(result.current.steps.map((s) => s.label)).toEqual([
      "Read backup",
      "Copy to staging",
      "Convert media",
      "Upload to vault",
    ]);
    expect(result.current.steps.map((s) => s.status)).toEqual([
      "done",
      "done",
      "pending",
      "pending",
    ]);
  });

  it("resumes at Gate 2 by diffing the STORED approved plan against a RECOMPUTED actual summary", async () => {
    const approved = stagingSummary({
      conversations: 3,
      verdictCounts: {
        fitsAsIs: 5,
        likelyFits: 0,
        mayGrow: 0,
        probablyTooBig: 0,
        cannotProcess: 0,
      },
    });
    const actual = stagingSummary({
      conversations: 3,
      verdictCounts: {
        fitsAsIs: 2,
        likelyFits: 0,
        mayGrow: 0,
        probablyTooBig: 0,
        cannotProcess: 0,
      },
    });
    invokeSummarizeStagingMock.mockResolvedValueOnce(actual);

    const { result } = renderHook(() => useImportJob());
    await act(async () => {
      await result.current.resumeAtGate(
        activeSession({ stage: "awaiting_gate_2", summary: approved }),
        form({ attachmentMedia: "convert" }),
      );
    });

    expect(invokeSummarizeStagingMock).toHaveBeenCalledTimes(1);
    expect(result.current.phase).toBe("gate_2");
    // What's shown is always the recomputed summary, never the stored one.
    expect(result.current.gateSummary).toEqual(actual);
    // And the delta is exactly what gateDelta(storedApproved, recomputed,
    // undefined) says — both inputs actually feed it, not just one.
    expect(result.current.gateDelta).toEqual(gateDelta(approved, actual, undefined));
    // Sanity: the two summaries genuinely differ, so a bug that fed the
    // same value in for both (or ignored the stored plan) would zero this.
    expect(result.current.gateDelta?.lostCount).toBeGreaterThan(0);
    // Decision 39: landing on a gate to look at it again writes nothing.
    expect(setImportStageMock).not.toHaveBeenCalled();
    // The media pass already ran (in an earlier session) to get here -- its
    // row shows done, not pending, and there are 4 of them.
    expect(result.current.steps).toHaveLength(4);
    expect(result.current.steps[2]).toMatchObject({ label: "Convert media", status: "done" });
  });

  it("re-runs the media pass on a resume at transcode, then lands on Gate 2", async () => {
    runMock.mockImplementationOnce(
      runResult({ summary: "Transcode finished.", transcode: undefined }),
    );
    const approved = stagingSummary({ conversations: 7 });
    invokeSummarizeStagingMock.mockResolvedValueOnce(stagingSummary({ conversations: 7 }));

    const { result } = renderHook(() => useImportJob());
    await act(async () => {
      await result.current.resumeAtGate(
        activeSession({ stage: "transcode", summary: approved }),
        form({ attachmentMedia: "convert" }),
      );
    });

    expect(invokeTranscodeStagingMock).toHaveBeenCalled();
    expect(result.current.phase).toBe("gate_2");
    // The stage write sequence matches the normal flow: "transcode" is
    // (idempotently) set again, then "awaiting_gate_2", both carrying the
    // plan stored at the last gate.
    expect(setImportStageMock).toHaveBeenCalledWith(1, "transcode", approved);
    expect(setImportStageMock).toHaveBeenCalledWith(1, "awaiting_gate_2", approved);
  });

  it("shows a 4-row list with the media row active while the pass re-runs on a transcode resume", async () => {
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
      resumed = result.current.resumeAtGate(
        activeSession({ stage: "transcode" }),
        form({ attachmentMedia: "convert" }),
      );
      // Let the microtasks up to (and including) invokeTranscodeStaging's
      // own call run, without waiting for `pending` to settle.
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(result.current.steps).toHaveLength(4);
    expect(result.current.steps.map((s) => s.status)).toEqual([
      "done",
      "done",
      "active",
      "pending",
    ]);

    await act(async () => {
      resolveTranscode({ summary: "Transcode finished.", transcode: undefined });
      await resumed;
    });
    expect(result.current.phase).toBe("gate_2");
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
      await result.current.resumeAtGate(
        activeSession({ stage: "transcode" }),
        form({ attachmentMedia: "convert" }),
      );
    });

    expect(invokeTranscodeStagingMock).not.toHaveBeenCalled();
    expect(result.current.phase).toBe("gate_1");
    expect(result.current.mediaToolsMissing).toBe(true);
    // The folder may hold a mix of originals and already-converted files --
    // Gate 1's "has not run yet" copy would be wrong here.
    expect(result.current.mediaPartiallyRan).toBe(true);
    expect(result.current.steps.map((s) => s.status)).toEqual([
      "done",
      "done",
      "pending",
      "pending",
    ]);
  });

  it("a malformed stored summary does not block a resume — it proceeds with no approved plan", async () => {
    const actual = stagingSummary({
      conversations: 4,
      verdictCounts: {
        fitsAsIs: 0,
        likelyFits: 1,
        mayGrow: 0,
        probablyTooBig: 0,
        cannotProcess: 0,
      },
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
      await result.current.resumeAtGate(
        activeSession({ stage: "awaiting_gate_2", summary: "not a valid staging summary" }),
        form({ attachmentMedia: "convert" }),
      );
    });

    expect(result.current.phase).toBe("gate_2");
    // No baseline to diff against: an unknown history reads as the mildest
    // severity, so the currently-flagged row shows as new information
    // instead of the resume silently blocking or throwing.
    expect(result.current.gateDelta).toEqual(gateDelta(undefined, actual, undefined));
    expect(result.current.gateDelta?.stillFlagged[0]?.regressed).toBe(true);
  });

  it("does nothing for a session at a stage this function doesn't handle", async () => {
    const { result } = renderHook(() => useImportJob());
    await act(async () => {
      await result.current.resumeAtGate(activeSession({ stage: "pushing" }), form());
    });

    expect(invokeSummarizeStagingMock).not.toHaveBeenCalled();
    expect(result.current.phase).toBe("form");
  });

  it.each(["awaiting_gate_1", "awaiting_gate_2"] as const)(
    "a recompute failure at %s does not complete the session or write a stage — it retries from the form",
    async (stage) => {
      invokeSummarizeStagingMock.mockRejectedValueOnce(new Error("disk unavailable"));

      const { result } = renderHook(() => useImportJob());
      await act(async () => {
        await result.current.resumeAtGate(
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
      await result.current.resumeAtGate(
        activeSession({ stage: "awaiting_gate_1" }),
        form({ attachmentMedia: "convert" }),
      );
    });
    expect(result.current.resumeError).not.toBeNull();

    invokeSummarizeStagingMock.mockResolvedValueOnce(stagingSummary());
    await act(async () => {
      await result.current.resumeAtGate(
        activeSession({ stage: "awaiting_gate_1" }),
        form({ attachmentMedia: "convert" }),
      );
    });
    expect(result.current.resumeError).toBeNull();
    expect(result.current.phase).toBe("gate_1");
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

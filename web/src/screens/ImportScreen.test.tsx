/** @vitest-environment jsdom */

// Entering Import must ask the vault whether a session is already live
// before showing anything: neither the blank form nor the resume panel
// may flash on screen while that check is in flight, and a vault that
// can't answer falls through to the form rather than blocking it.

import { act, cleanup, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ActiveImportSession } from "../lib/importSession";
import type { StagingSummary } from "../lib/tauri";
import { mockedAuth, renderWithVault } from "../test/vaultProviders";
import type { GateDelta } from "./import/gateDelta";
import type { ResumeDecision } from "./import/resumeDecision";

const hookState = vi.hoisted(() => ({
  phase: "form" as
    | "form"
    | "running"
    | "staging_approval"
    | "media_approval"
    | "done"
    | "identity_stop",
  stagingSummary: null as StagingSummary | null,
  mediaDelta: null as GateDelta | null,
  mediaToolsMissing: false,
  mediaPartiallyRan: false,
  resumeError: null as string | null,
  sourceIdentities: null as string[] | null,
  approvalDismissed: false,
}));
const startImportMock = vi.hoisted(() => vi.fn());
const resumeAtGateMock = vi.hoisted(() => vi.fn());
const approveMock = vi.hoisted(() => vi.fn());
const cancelRunMock = vi.hoisted(() => vi.fn());
const dismissApprovalMock = vi.hoisted(() => vi.fn());
const reviewApprovalMock = vi.hoisted(() => vi.fn());
const cancelMock = vi.hoisted(() => vi.fn());
const returnToFormMock = vi.hoisted(() => vi.fn());
const continueAfterIdentityStopMock = vi.hoisted(() => vi.fn());
const cancelIdentityStopMock = vi.hoisted(() => vi.fn());
const getActiveImportSessionMock = vi.hoisted(() => vi.fn());
const discardImportSessionMock = vi.hoisted(() => vi.fn());
const invokeDeleteStagingMock = vi.hoisted(() => vi.fn());
const invokePathStatMock = vi.hoisted(() => vi.fn());
const apiPostMock = vi.hoisted(() => vi.fn());
const apiGetMock = vi.hoisted(() => vi.fn());

vi.mock("./import/useImportJob", async (importOriginal) => {
  // Only useImportJob itself is replaced; parseStoredStagingSummary stays
  // real. restoreFormFromSnapshot now lives in formSnapshot.ts, which is not
  // mocked at all, so the screen runs the same validation it does in
  // production.
  const actual = await importOriginal<typeof import("./import/useImportJob")>();
  return {
    ...actual,
    useImportJob: () => ({
      phase: hookState.phase,
      steps: [],
      running: false,
      form: null,
      summaryView: null,
      stagingDir: null,
      importSessionId: null,
      stagingSummary: hookState.stagingSummary,
      mediaDelta: hookState.mediaDelta,
      attachmentMedia: "copy",
      mediaToolsMissing: hookState.mediaToolsMissing,
      mediaPartiallyRan: hookState.mediaPartiallyRan,
      resumeError: hookState.resumeError,
      sourceIdentities: hookState.sourceIdentities,
      computingSummary: false,
      completionText: undefined,
      approvalDismissed: hookState.approvalDismissed,
      startImport: startImportMock,
      resumeAtGate: resumeAtGateMock,
      approve: approveMock,
      cancelRun: cancelRunMock,
      dismissApproval: dismissApprovalMock,
      reviewApproval: reviewApprovalMock,
      cancel: cancelMock,
      returnToForm: returnToFormMock,
      continueAfterIdentityStop: continueAfterIdentityStopMock,
      cancelIdentityStop: cancelIdentityStopMock,
    }),
  };
});

vi.mock("../lib/importSession", () => ({
  getActiveImportSession: (...args: unknown[]) => getActiveImportSessionMock(...args),
  discardImportSession: (...args: unknown[]) => discardImportSessionMock(...args),
}));

vi.mock("../lib/deviceId", () => ({
  getDeviceId: () => "this-device",
}));

// The three vault calls this screen makes, faked by name. The rest of
// vaultApi stays real, since other modules in this graph import from it.
vi.mock("../lib/vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/vaultApi")>()),
  unmatchedHandles: (...args: unknown[]) => apiPostMock(...args),
  updateAccountProfile: (...args: unknown[]) => apiPostMock(...args),
  getAccountProfile: (...args: unknown[]) => apiGetMock(...args),
}));

vi.mock("../lib/tauri", () => ({
  invokeHomeDir: vi.fn().mockResolvedValue({ path: "/home/u", os: "linux" }),
  invokeIosBackupEncrypted: vi.fn().mockResolvedValue(null),
  invokePathStat: (...args: unknown[]) => invokePathStatMock(...args),
  invokeDeleteStaging: (...args: unknown[]) => invokeDeleteStagingMock(...args),
}));

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("../lib/tauri-check", () => ({
  isTauri: () => false,
}));

vi.mock("./import/ImportFormFields", () => ({
  default: () => <div data-testid="import-form" />,
}));

vi.mock("./import/ImportRunView", () => ({
  default: (props: { approvalWaiting: string | null; onReview: () => void }) => (
    <div data-testid="import-run">
      <span data-testid="run-approval-waiting">{String(props.approvalWaiting)}</span>
      <button type="button" onClick={props.onReview}>
        run-review
      </button>
    </div>
  ),
}));

vi.mock("./import/ResumeImportPanel", () => ({
  default: (props: {
    decision: ResumeDecision;
    error?: string | null;
    onResume: () => void;
    onDiscard: () => void;
  }) => (
    <div data-testid="resume-panel">
      <span data-testid="resume-kind">{props.decision.kind}</span>
      {props.error ? <span data-testid="resume-error">{props.error}</span> : null}
      <button type="button" onClick={props.onResume}>
        resume-action
      </button>
      <button type="button" onClick={props.onDiscard}>
        discard-action
      </button>
    </div>
  ),
}));

vi.mock("./import/ImportApprovalScreen", () => ({
  default: (props: {
    kind: "staging" | "media";
    summary: StagingSummary;
    unknownContacts: number | null;
    onApprove: () => void;
    onCancel: () => void;
    onBack: () => void;
  }) => (
    <div data-testid="approval">
      <span data-testid="approval-kind">{props.kind}</span>
      <span data-testid="approval-unknown-contacts">{String(props.unknownContacts)}</span>
      <button type="button" onClick={props.onApprove}>
        approval-approve
      </button>
      <button type="button" onClick={props.onCancel}>
        approval-cancel
      </button>
      <button type="button" onClick={props.onBack}>
        approval-back
      </button>
    </div>
  ),
}));

const { default: ImportScreen } = await import("./ImportScreen");

function stagingSummary(overrides: Partial<StagingSummary> = {}): StagingSummary {
  return {
    conversations: 1,
    messages: 1,
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

function session(overrides: Partial<ActiveImportSession> = {}): ActiveImportSession {
  return {
    id: 7,
    source: "imessage",
    mode: "append",
    status: "running",
    started_at: "2026-08-30T00:00:00Z",
    stage: "pushing",
    staging_dir: "/home/u/message-vault/staging-260830",
    device_id: "this-device",
    form: { source: "imessage-ios" },
    source_fingerprint: null,
    source_identities: null,
    summary: null,
    ...overrides,
  };
}

/** A promise plus the functions to settle it later, for controlling when the check resolves. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("ImportScreen entering Import", () => {
  beforeEach(() => {
    hookState.phase = "form";
    hookState.stagingSummary = null;
    hookState.mediaDelta = null;
    hookState.approvalDismissed = false;
    hookState.mediaToolsMissing = false;
    hookState.mediaPartiallyRan = false;
    hookState.resumeError = null;
    hookState.sourceIdentities = null;
    startImportMock.mockReset();
    resumeAtGateMock.mockReset();
    resumeAtGateMock.mockResolvedValue(undefined);
    approveMock.mockReset();
    cancelRunMock.mockReset();
    cancelMock.mockReset();
    returnToFormMock.mockReset();
    continueAfterIdentityStopMock.mockReset();
    cancelIdentityStopMock.mockReset();
    getActiveImportSessionMock.mockReset();
    discardImportSessionMock.mockReset();
    discardImportSessionMock.mockResolvedValue(undefined);
    invokeDeleteStagingMock.mockReset();
    invokeDeleteStagingMock.mockResolvedValue(undefined);
    invokePathStatMock.mockReset();
    invokePathStatMock.mockResolvedValue({ exists: true, isFile: false, isDirectory: true });
    apiPostMock.mockReset();
    apiPostMock.mockResolvedValue({ items: [], total: 0, limit: 500, offset: 0 });
    apiGetMock.mockReset();
    apiGetMock.mockResolvedValue({
      account_id: 7,
      username: "demo",
      preferred_name: null,
      phones: [],
      emails: [],
    });
  });

  afterEach(() => {
    cleanup();
  });

  it("renders neither the form nor a panel while the active-session check is in flight", async () => {
    const pending = deferred<ActiveImportSession | null>();
    getActiveImportSessionMock.mockReturnValue(pending.promise);

    renderWithVault(<ImportScreen />);

    expect(screen.queryByTestId("import-form")).not.toBeInTheDocument();
    expect(screen.queryByTestId("resume-panel")).not.toBeInTheDocument();

    await act(async () => {
      pending.resolve(null);
      await pending.promise;
    });

    expect(screen.getByTestId("import-form")).toBeInTheDocument();
  });

  it("shows the form when there is no active session", async () => {
    getActiveImportSessionMock.mockResolvedValue(null);
    renderWithVault(<ImportScreen />);

    expect(await screen.findByTestId("import-form")).toBeInTheDocument();
    expect(screen.queryByTestId("resume-panel")).not.toBeInTheDocument();
  });

  it("falls through to the form when the vault cannot answer", async () => {
    getActiveImportSessionMock.mockRejectedValue(new Error("network down"));
    renderWithVault(<ImportScreen />);

    expect(await screen.findByTestId("import-form")).toBeInTheDocument();
    expect(screen.queryByTestId("resume-panel")).not.toBeInTheDocument();
  });

  it("shows the resume panel instead of the form for a resumable session", async () => {
    getActiveImportSessionMock.mockResolvedValue(session({ stage: "pushing" }));
    renderWithVault(<ImportScreen />);

    expect(await screen.findByTestId("resume-panel")).toBeInTheDocument();
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("resume_push");
    expect(screen.queryByTestId("import-form")).not.toBeInTheDocument();
  });

  it("says the staging folder could not be checked, not that it is gone, when the stat fails", async () => {
    getActiveImportSessionMock.mockResolvedValue(session({ stage: "pushing" }));
    invokePathStatMock.mockRejectedValue(new Error("ipc down"));
    renderWithVault(<ImportScreen />);

    expect(await screen.findByTestId("resume-panel")).toBeInTheDocument();
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("folder_unknown");
  });

  it("discards the session and drops through to the form", async () => {
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(session({ stage: "pushing" }));
    renderWithVault(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    await user.click(screen.getByText("discard-action"));

    expect(discardImportSessionMock).toHaveBeenCalledWith(7);
    expect(await screen.findByTestId("import-form")).toBeInTheDocument();
  });

  it("also deletes the staging folder when discarding a this-device session", async () => {
    // W7: declineGate already deletes the staging folder on decline
    // (decision 16) -- a panel discard is the same operation reached
    // through a different button, and used to only call
    // discardImportSession, orphaning a potentially multi-GB folder.
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(
      session({
        stage: "pushing",
        device_id: "this-device",
        staging_dir: "/home/u/message-vault/staging-260830",
      }),
    );
    renderWithVault(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    await user.click(screen.getByText("discard-action"));

    expect(discardImportSessionMock).toHaveBeenCalledWith(7);
    expect(invokeDeleteStagingMock).toHaveBeenCalledWith({
      staging_dir: "/home/u/message-vault/staging-260830",
    });
    expect(await screen.findByTestId("import-form")).toBeInTheDocument();
  });

  it("never touches disk when discarding another device's session", async () => {
    // resumeDecisionFor routes an other-device session to "other_device",
    // whose files are staged on that other install, not here -- deleting a
    // local path with the same name would be wrong, or a no-op at best.
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(
      session({
        stage: "pushing",
        device_id: "another-device",
        staging_dir: "/home/u/message-vault/staging-260830",
      }),
    );
    renderWithVault(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("other_device");
    await user.click(screen.getByText("discard-action"));

    expect(discardImportSessionMock).toHaveBeenCalledWith(7);
    expect(invokeDeleteStagingMock).not.toHaveBeenCalled();
    expect(await screen.findByTestId("import-form")).toBeInTheDocument();
  });

  it("resumes the push against the existing session without creating a new one", async () => {
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(
      session({
        stage: "pushing",
        staging_dir: "/home/u/message-vault/staging-260830",
        form: {
          source: "imessage-ios",
          backupPath: "/backups/iphone.tar",
          attachmentMedia: "copy",
          maxResolution: "720p",
          maxFps: "30",
          minSizeMb: "20",
          ownerPhones: [],
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
        },
      }),
    );
    renderWithVault(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    await user.click(screen.getByText("resume-action"));

    expect(startImportMock).toHaveBeenCalledTimes(1);
    const [form, resume] = startImportMock.mock.calls[0] as [unknown, unknown];
    expect(form).toMatchObject({ source: "imessage-ios", backupPath: "/backups/iphone.tar" });
    expect(resume).toEqual({ sessionId: 7, stagingDir: "/home/u/message-vault/staging-260830" });
    expect(discardImportSessionMock).not.toHaveBeenCalled();
  });

  const restorableForm = {
    source: "imessage-ios",
    backupPath: "/backups/iphone.tar",
    attachmentMedia: "copy",
    maxResolution: "720p",
    maxFps: "30",
    minSizeMb: "20",
    ownerPhones: [],
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

  it.each([
    ["awaiting_gate_1", "resume_gate"],
    ["awaiting_gate_2", "resume_gate"],
    ["transcode", "resume_media"],
  ] as const)(
    "routes a session at %s through resumeAtGate, not startImport or discard",
    async (stage, kind) => {
      const user = userEvent.setup();
      getActiveImportSessionMock.mockResolvedValue(
        session({
          stage,
          staging_dir: "/home/u/message-vault/staging-260830",
          form: restorableForm,
        }),
      );
      renderWithVault(<ImportScreen />);

      await screen.findByTestId("resume-panel");
      expect(screen.getByTestId("resume-kind")).toHaveTextContent(kind);
      await user.click(screen.getByText("resume-action"));

      expect(resumeAtGateMock).toHaveBeenCalledTimes(1);
      const [resumedSession, resumedForm] = resumeAtGateMock.mock.calls[0] as [
        ActiveImportSession,
        unknown,
      ];
      expect(resumedSession.id).toBe(7);
      expect(resumedSession.stage).toBe(stage);
      // The screen's own already-validated parse, not a second one inside
      // the hook.
      expect(resumedForm).toMatchObject({ source: "imessage-ios", attachmentMedia: "copy" });
      expect(startImportMock).not.toHaveBeenCalled();
      expect(discardImportSessionMock).not.toHaveBeenCalled();
    },
  );

  it("re-fetches and reshows the resume panel with the failure surfaced when a gate resume's recompute fails", async () => {
    // Decision 37: only an explicit discard ends a waiting session, so a
    // failed recompute (useImportJob's resumeAtGate) never completes or
    // discards it -- it returns to the form phase instead. That phase
    // transition is what re-triggers this screen's own active-session
    // check, and since nothing was touched server-side, it finds the exact
    // same session and shows the panel again -- this is the retry.
    getActiveImportSessionMock.mockResolvedValue(
      session({ stage: "awaiting_gate_1", form: restorableForm }),
    );
    const { rerender } = renderWithVault(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("resume_gate");
    expect(getActiveImportSessionMock).toHaveBeenCalledTimes(1);

    // Simulate resumeAtGate's failure path from inside the (mocked) hook:
    // phase moves to "running" while it recomputes, then back to "form"
    // with the failure left on `resumeError`.
    hookState.phase = "running";
    await act(async () => {
      rerender(<ImportScreen />);
    });
    hookState.phase = "form";
    hookState.resumeError = "disk unavailable";
    await act(async () => {
      rerender(<ImportScreen />);
    });

    expect(getActiveImportSessionMock).toHaveBeenCalledTimes(2);
    expect(await screen.findByTestId("resume-panel")).toBeInTheDocument();
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("resume_gate");
    expect(screen.getByTestId("resume-error")).toHaveTextContent("disk unavailable");
  });

  it("discards the old session before restarting when the extract never finished", async () => {
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(
      session({
        stage: "parse",
        form: {
          source: "imessage-ios",
          backupPath: "/backups/iphone.tar",
          attachmentMedia: "copy",
          maxResolution: "720p",
          maxFps: "30",
          minSizeMb: "20",
          ownerPhones: [],
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
        },
      }),
    );
    renderWithVault(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("restart");
    await user.click(screen.getByText("resume-action"));

    expect(discardImportSessionMock).toHaveBeenCalledWith(7);
    // The old folder goes with the session: a restart writes into a new one,
    // and nothing will ever reach this one again.
    expect(invokeDeleteStagingMock).toHaveBeenCalledWith({
      staging_dir: "/home/u/message-vault/staging-260830",
    });
    expect(startImportMock).toHaveBeenCalledTimes(1);
    const [form, resume] = startImportMock.mock.calls[0] as [unknown, unknown];
    expect(form).toMatchObject({ source: "imessage-ios", backupPath: "/backups/iphone.tar" });
    expect(resume).toBeUndefined();
  });

  it("picks up an interrupted copy in the folder it was already writing into", async () => {
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(
      session({
        stage: "write",
        source_fingerprint: {
          path: "/backups/iphone.tar",
          size_bytes: 1000,
          modified_unix_ms: 1_700_000_000_000,
          message_count: null,
        },
        form: {
          source: "imessage-ios",
          backupPath: "/backups/iphone.tar",
          attachmentMedia: "copy",
          maxResolution: "720p",
          maxFps: "30",
          minSizeMb: "20",
          ownerPhones: [],
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
        },
      }),
    );
    // The staged folder first, then the backup: same size and mtime, so the
    // fingerprint matches and the copy is safe to continue.
    invokePathStatMock
      .mockResolvedValueOnce({ exists: true, isFile: false, isDirectory: true })
      .mockResolvedValueOnce({
        exists: true,
        isFile: true,
        isDirectory: false,
        sizeBytes: 1000,
        modifiedUnixMs: 1_700_000_000_000,
      });
    renderWithVault(<ImportScreen />);

    expect(await screen.findByTestId("resume-panel")).toBeInTheDocument();
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("resume_write");

    await user.click(screen.getByText("resume-action"));

    expect(discardImportSessionMock).not.toHaveBeenCalled();
    expect(invokeDeleteStagingMock).not.toHaveBeenCalled();
    expect(startImportMock).toHaveBeenCalledTimes(1);
    const [, resume, resumeWrite] = startImportMock.mock.calls[0] as [unknown, unknown, unknown];
    expect(resume).toBeUndefined();
    expect(resumeWrite).toEqual({
      sessionId: 7,
      stagingDir: "/home/u/message-vault/staging-260830",
      identities: null,
    });
  });

  it("says the backup changed when its size no longer matches what was recorded", async () => {
    getActiveImportSessionMock.mockResolvedValue(
      session({
        stage: "write",
        source_fingerprint: {
          path: "/backups/iphone.tar",
          size_bytes: 1000,
          modified_unix_ms: 1_700_000_000_000,
          message_count: null,
        },
      }),
    );
    invokePathStatMock
      .mockResolvedValueOnce({ exists: true, isFile: false, isDirectory: true })
      .mockResolvedValueOnce({
        exists: true,
        isFile: true,
        isDirectory: false,
        sizeBytes: 999_999,
        modifiedUnixMs: 1_700_000_000_000,
      });
    renderWithVault(<ImportScreen />);

    expect(await screen.findByTestId("resume-panel")).toBeInTheDocument();
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("source_changed");
  });

  it("re-checks for an open session when the screen returns to the form", async () => {
    // A swallowed final /complete, or a restart whose discard failed,
    // leaves a session open server-side that the screen has forgotten. If
    // Back never re-checks, the user gets a form whose Import button 409s.
    getActiveImportSessionMock.mockResolvedValue(null);
    const { rerender } = renderWithVault(<ImportScreen />);

    expect(await screen.findByTestId("import-form")).toBeInTheDocument();
    expect(getActiveImportSessionMock).toHaveBeenCalledTimes(1);

    hookState.phase = "running";
    await act(async () => {
      rerender(<ImportScreen />);
    });
    expect(getActiveImportSessionMock).toHaveBeenCalledTimes(1);

    hookState.phase = "done";
    await act(async () => {
      rerender(<ImportScreen />);
    });
    expect(getActiveImportSessionMock).toHaveBeenCalledTimes(1);

    getActiveImportSessionMock.mockResolvedValue(session({ stage: "pushing" }));
    hookState.phase = "form";
    await act(async () => {
      rerender(<ImportScreen />);
    });

    expect(getActiveImportSessionMock).toHaveBeenCalledTimes(2);
    expect(await screen.findByTestId("resume-panel")).toBeInTheDocument();
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("resume_push");
  });

  it("runs one restart when the resume action is double-clicked", async () => {
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(
      session({
        stage: "parse",
        form: {
          source: "imessage-ios",
          backupPath: "/backups/iphone.tar",
          attachmentMedia: "copy",
          maxResolution: "720p",
          maxFps: "30",
          minSizeMb: "20",
          ownerPhones: [],
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
        },
      }),
    );
    // The panel stays mounted across this round trip by design, so the
    // second click lands on a live button.
    const pendingDiscard = deferred<void>();
    discardImportSessionMock.mockReturnValue(pendingDiscard.promise);
    renderWithVault(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    await user.click(screen.getByText("resume-action"));
    await user.click(screen.getByText("resume-action"));
    await user.click(screen.getByText("discard-action"));

    expect(discardImportSessionMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      pendingDiscard.resolve();
      await pendingDiscard.promise;
    });

    expect(discardImportSessionMock).toHaveBeenCalledTimes(1);
    expect(startImportMock).toHaveBeenCalledTimes(1);
  });

  it("falls back to a settings-unreadable panel when the stored form snapshot is malformed", async () => {
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(
      session({ stage: "pushing", form: { nonsense: true } }),
    );
    renderWithVault(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("resume_push");
    await user.click(screen.getByText("resume-action"));

    expect(startImportMock).not.toHaveBeenCalled();
    expect(await screen.findByTestId("resume-kind")).toHaveTextContent("settings_unreadable");
  });

  it("still drops to the form when discarding from the panel fails server-side", async () => {
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(session({ stage: "pushing" }));
    discardImportSessionMock.mockRejectedValue(new Error("network down"));
    renderWithVault(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    await user.click(screen.getByText("discard-action"));

    expect(discardImportSessionMock).toHaveBeenCalledWith(7);
    expect(await screen.findByTestId("import-form")).toBeInTheDocument();
  });

  it("still restarts when discarding the old session before a restart fails server-side", async () => {
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(
      session({
        stage: "parse",
        form: {
          source: "imessage-ios",
          backupPath: "/backups/iphone.tar",
          attachmentMedia: "copy",
          maxResolution: "720p",
          maxFps: "30",
          minSizeMb: "20",
          ownerPhones: [],
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
        },
      }),
    );
    discardImportSessionMock.mockRejectedValue(new Error("network down"));
    renderWithVault(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    await user.click(screen.getByText("resume-action"));

    expect(discardImportSessionMock).toHaveBeenCalledWith(7);
    expect(startImportMock).toHaveBeenCalledTimes(1);
    const [form, resume] = startImportMock.mock.calls[0] as [unknown, unknown];
    expect(form).toMatchObject({ source: "imessage-ios", backupPath: "/backups/iphone.tar" });
    expect(resume).toBeUndefined();
  });
});

describe("ImportScreen gates", () => {
  beforeEach(() => {
    hookState.phase = "form";
    hookState.stagingSummary = null;
    hookState.mediaDelta = null;
    hookState.approvalDismissed = false;
    hookState.mediaToolsMissing = false;
    hookState.mediaPartiallyRan = false;
    hookState.resumeError = null;
    hookState.sourceIdentities = null;
    startImportMock.mockReset();
    resumeAtGateMock.mockReset();
    resumeAtGateMock.mockResolvedValue(undefined);
    approveMock.mockReset();
    cancelRunMock.mockReset();
    cancelMock.mockReset();
    returnToFormMock.mockReset();
    continueAfterIdentityStopMock.mockReset();
    cancelIdentityStopMock.mockReset();
    getActiveImportSessionMock.mockReset();
    getActiveImportSessionMock.mockResolvedValue(null);
    discardImportSessionMock.mockReset();
    invokePathStatMock.mockReset();
    apiPostMock.mockReset();
    apiPostMock.mockResolvedValue({ items: [], total: 0, limit: 500, offset: 0 });
    apiGetMock.mockReset();
    apiGetMock.mockResolvedValue({
      account_id: 7,
      username: "demo",
      preferred_name: null,
      phones: [],
      emails: [],
    });
  });

  afterEach(() => {
    cleanup();
  });

  it("renders the Staging Approval with the summary and wires approve and cancel through to the hook", async () => {
    hookState.phase = "staging_approval";
    hookState.stagingSummary = stagingSummary({ contactIdentifiers: ["+15551234567"] });
    const user = userEvent.setup();
    renderWithVault(<ImportScreen />);

    expect(await screen.findByTestId("approval")).toBeInTheDocument();
    expect(screen.queryByTestId("import-form")).not.toBeInTheDocument();

    await user.click(screen.getByText("approval-approve"));
    expect(approveMock).toHaveBeenCalledTimes(1);

    await user.click(screen.getByText("approval-cancel"));
    expect(cancelRunMock).toHaveBeenCalledTimes(1);
  });

  it("shows the run with the approval waiting once the person steps back from it", async () => {
    hookState.phase = "staging_approval";
    hookState.stagingSummary = stagingSummary();
    hookState.approvalDismissed = true;
    const user = userEvent.setup();
    renderWithVault(<ImportScreen />);

    expect(await screen.findByTestId("import-run")).toBeInTheDocument();
    expect(screen.queryByTestId("approval")).not.toBeInTheDocument();
    expect(screen.getByTestId("run-approval-waiting")).toHaveTextContent("staging");

    await user.click(screen.getByText("run-review"));
    expect(reviewApprovalMock).toHaveBeenCalledTimes(1);
  });

  it("lets the person step back from the approval without deciding", async () => {
    hookState.phase = "media_approval";
    hookState.stagingSummary = stagingSummary();
    hookState.mediaDelta = { lostCount: 0, stillFlagged: [], cameOutFine: 0, hasChanges: false };
    const user = userEvent.setup();
    renderWithVault(<ImportScreen />);

    expect(await screen.findByTestId("approval")).toBeInTheDocument();
    expect(screen.getByTestId("approval-kind")).toHaveTextContent("media");
    await user.click(screen.getByText("approval-back"));
    expect(dismissApprovalMock).toHaveBeenCalledTimes(1);
    expect(approveMock).not.toHaveBeenCalled();
    expect(cancelRunMock).not.toHaveBeenCalled();
  });

  it("renders the Media Approval with the delta and wires approve and cancel through to the hook", async () => {
    hookState.phase = "media_approval";
    hookState.stagingSummary = stagingSummary();
    hookState.mediaDelta = { lostCount: 0, stillFlagged: [], cameOutFine: 0, hasChanges: false };
    const user = userEvent.setup();
    renderWithVault(<ImportScreen />);

    expect(await screen.findByTestId("approval")).toBeInTheDocument();

    await user.click(screen.getByText("approval-approve"));
    expect(approveMock).toHaveBeenCalledTimes(1);

    await user.click(screen.getByText("approval-cancel"));
    expect(cancelRunMock).toHaveBeenCalledTimes(1);
  });

  it("looks up which of the staged contacts are unknown, in one batch under the server cap", async () => {
    hookState.phase = "staging_approval";
    hookState.stagingSummary = stagingSummary({ contactIdentifiers: ["a", "b", "c"] });
    apiPostMock.mockResolvedValue({ items: ["a", "c"], total: 2, limit: 500, offset: 0 });
    renderWithVault(<ImportScreen />);

    await screen.findByTestId("approval");
    await act(async () => {
      await Promise.resolve();
    });

    expect(apiPostMock).toHaveBeenCalledTimes(1);
    expect(apiPostMock).toHaveBeenCalledWith({ identifiers: ["a", "b", "c"] });
    expect(screen.getByTestId("approval-unknown-contacts")).toHaveTextContent("2");
  });

  it("batches the contact-match lookup at 500 identifiers per request and sums unknown across batches", async () => {
    hookState.phase = "staging_approval";
    const identifiers = Array.from({ length: 620 }, (_, i) => `+1555000${i}`);
    hookState.stagingSummary = stagingSummary({ contactIdentifiers: identifiers });
    apiPostMock.mockResolvedValueOnce({
      items: Array(400).fill("x"),
      total: 400,
      limit: 500,
      offset: 0,
    });
    apiPostMock.mockResolvedValueOnce({
      items: Array(30).fill("y"),
      total: 30,
      limit: 500,
      offset: 0,
    });
    renderWithVault(<ImportScreen />);

    await screen.findByTestId("approval");
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(apiPostMock).toHaveBeenCalledTimes(2);
    const bodies = apiPostMock.mock.calls.map(([body]) => body as { identifiers: string[] });
    expect(bodies[0]?.identifiers).toHaveLength(500);
    expect(bodies[1]?.identifiers).toHaveLength(120);
    expect(screen.getByTestId("approval-unknown-contacts")).toHaveTextContent("430");
  });

  it("renders the approval without the unknown-contact count when the lookup fails", async () => {
    hookState.phase = "staging_approval";
    hookState.stagingSummary = stagingSummary({ contactIdentifiers: ["a"] });
    apiPostMock.mockRejectedValue(new Error("network down"));
    renderWithVault(<ImportScreen />);

    await screen.findByTestId("approval");
    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.getByTestId("approval-unknown-contacts")).toHaveTextContent("null");
  });

  it("shows the identity stop screen for the identity_stop phase", async () => {
    hookState.phase = "identity_stop";
    hookState.sourceIdentities = ["+15550001111"];
    renderWithVault(<ImportScreen />);

    expect(
      await screen.findByText("None of the addresses this backup sent from are on your profile."),
    ).toBeInTheDocument();
  });

  it("shows a factual line when adding an identity to the profile fails", async () => {
    hookState.phase = "identity_stop";
    hookState.sourceIdentities = ["+15550001111"];
    apiPostMock.mockRejectedValue(new Error("network down"));
    const user = userEvent.setup();
    renderWithVault(<ImportScreen />);

    await user.click(await screen.findByText("Add to profile"));

    expect(await screen.findByText("The vault didn't add that address.")).toBeInTheDocument();
  });
});

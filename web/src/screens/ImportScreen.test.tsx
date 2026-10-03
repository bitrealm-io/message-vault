/** @vitest-environment jsdom */

// Entering Import must ask the server whether a session is already live
// before showing anything: neither the blank form nor the resume panel
// may flash on screen while that check is in flight, and a server that
// can't answer falls through to the form rather than blocking it.

import { act, cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ActiveImportSession } from "../lib/importSession";
import type { StagingSummary } from "../lib/tauri";
import { mockedAuth, renderWithProviders } from "../test/providers";
import type { ResumeDecision } from "./import/resumeDecision";

const hookState = vi.hoisted(() => ({
  phase: "form" as
    | "form"
    | "running"
    | "staging_review"
    | "media_review"
    | "done"
    | "identity_stop",
  stagingSummary: null as StagingSummary | null,
  mediaSummary: null as StagingSummary | null,
  mediaFailedCount: null as number | null,
  mediaToolsMissing: false,
  mediaPartiallyRan: false,
  resumeError: null as string | null,
  sourceIdentities: null as string[] | null,
}));
const startImportMock = vi.hoisted(() => vi.fn());
const resumeAtReviewMock = vi.hoisted(() => vi.fn());
const approveMock = vi.hoisted(() => vi.fn());
const cancelRunMock = vi.hoisted(() => vi.fn());
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
const listImportsMock = vi.hoisted(() => vi.fn());

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
      mediaSummary: hookState.mediaSummary,
      mediaFailedCount: hookState.mediaFailedCount,
      mediaToolsMissing: hookState.mediaToolsMissing,
      mediaPartiallyRan: hookState.mediaPartiallyRan,
      resumeError: hookState.resumeError,
      sourceIdentities: hookState.sourceIdentities,
      computingSummary: false,
      completionText: undefined,
      startImport: startImportMock,
      resumeAtReview: resumeAtReviewMock,
      approve: approveMock,
      cancelRun: cancelRunMock,
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

// The server calls this screen makes, faked by name. The rest of
// serverApi stays real, since other modules in this graph import from it.
vi.mock("../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/serverApi")>()),
  listImports: (...args: unknown[]) => listImportsMock(...args),
  unmatchedIdentities: (...args: unknown[]) => apiPostMock(...args),
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
  default: (props: {
    reviewWaiting: string | null;
    unknownContacts: number | null;
    unknownContactsError: string | null;
    identityPanel?: unknown;
    onApprove: () => void;
    onCancelRun: () => void;
    onBack: () => void;
  }) => (
    <div data-testid="import-run">
      <span data-testid="run-review-waiting">{String(props.reviewWaiting)}</span>
      <span data-testid="run-unknown-contacts">{String(props.unknownContacts)}</span>
      <span data-testid="run-unknown-contacts-error">{String(props.unknownContactsError)}</span>
      <span data-testid="run-has-identities">{String(props.identityPanel != null)}</span>
      <button type="button" onClick={props.onApprove}>
        run-approve
      </button>
      <button type="button" onClick={props.onCancelRun}>
        run-cancel-run
      </button>
      <button type="button" onClick={props.onBack}>
        run-back
      </button>
    </div>
  ),
}));

vi.mock("./import/ResumeImportPanel", () => ({
  default: (props: {
    decision: ResumeDecision;
    secret?: string | null;
    error?: string | null;
    onResume: (secret: string) => void;
    onDiscard: () => void;
  }) => (
    <div data-testid="resume-panel">
      <span data-testid="resume-kind">{props.decision.kind}</span>
      <span data-testid="resume-secret">{props.secret ?? "none"}</span>
      {props.error ? <span data-testid="resume-error">{props.error}</span> : null}
      <button type="button" onClick={() => props.onResume("typed-secret")}>
        resume-action
      </button>
      <button type="button" onClick={props.onDiscard}>
        discard-action
      </button>
    </div>
  ),
}));

const { default: ImportScreen } = await import("./ImportScreen");
const { useImportAttention } = await import("./import/useImportAttention");

function stagingSummary(overrides: Partial<StagingSummary> = {}): StagingSummary {
  return {
    conversations: 1,
    messages: 1,
    contactIdentifiers: [],
    ownerHandles: [],
    attachments: 0,
    attachmentBytes: 0,
    forecasts: [],
    assetMaxBytes: 50 * 1024 * 1024,
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
    stage: "upload",
    staging_dir: "/home/u/message-crate/staging-260830",
    device_id: "this-device",
    form: { source: "imessage-ios" },
    source_fingerprint: null,
    source_identities: null,
    summary: null,
    ...overrides,
  };
}

/** A stored form snapshot for an iPhone backup, as `formSnapshot` writes it. */
function storedForm(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    source: "imessage-ios",
    backupPath: "/backups/iphone",
    attachmentMedia: "copy",
    maxResolution: "720p",
    maxFps: "30",
    minSizeMb: "20",
    ownerPhones: [],
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
    assetMaxBytes: 512 * 1024 * 1024,
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
    hookState.mediaSummary = null;
    hookState.mediaFailedCount = null;
    hookState.mediaToolsMissing = false;
    hookState.mediaPartiallyRan = false;
    hookState.resumeError = null;
    hookState.sourceIdentities = null;
    startImportMock.mockReset();
    resumeAtReviewMock.mockReset();
    resumeAtReviewMock.mockResolvedValue(undefined);
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

    renderWithProviders(<ImportScreen />);

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
    renderWithProviders(<ImportScreen />);

    expect(await screen.findByTestId("import-form")).toBeInTheDocument();
    expect(screen.queryByTestId("resume-panel")).not.toBeInTheDocument();
  });

  it("falls through to the form when the server cannot answer", async () => {
    getActiveImportSessionMock.mockRejectedValue(new Error("network down"));
    renderWithProviders(<ImportScreen />);

    expect(await screen.findByTestId("import-form")).toBeInTheDocument();
    expect(screen.queryByTestId("resume-panel")).not.toBeInTheDocument();
  });

  it("shows the resume panel instead of the form for a resumable session", async () => {
    getActiveImportSessionMock.mockResolvedValue(session({ stage: "upload" }));
    renderWithProviders(<ImportScreen />);

    expect(await screen.findByTestId("resume-panel")).toBeInTheDocument();
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("resume_push");
    expect(screen.queryByTestId("import-form")).not.toBeInTheDocument();
  });

  it("says the staging folder could not be checked, not that it is gone, when the stat fails", async () => {
    getActiveImportSessionMock.mockResolvedValue(session({ stage: "upload" }));
    invokePathStatMock.mockRejectedValue(new Error("ipc down"));
    renderWithProviders(<ImportScreen />);

    expect(await screen.findByTestId("resume-panel")).toBeInTheDocument();
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("folder_unknown");
  });

  it("discards the session and drops through to the form", async () => {
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(session({ stage: "upload" }));
    renderWithProviders(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    await user.click(screen.getByText("discard-action"));

    expect(discardImportSessionMock).toHaveBeenCalledWith(7);
    expect(await screen.findByTestId("import-form")).toBeInTheDocument();
  });

  it("takes the sidebar's waiting badge away when the waiting run is discarded", async () => {
    // A run left waiting at the Staging Review before the app was closed is
    // only on the server, so the badge reads it from there. Discard ends it,
    // and the badge must go at once rather than on a later refetch.
    const user = userEvent.setup();
    let running: ActiveImportSession | null = session({ stage: "staging_review" });
    getActiveImportSessionMock.mockImplementation(async () => running);
    listImportsMock.mockImplementation(async () => ({
      items: running ? [running] : [],
      total: running ? 1 : 0,
      limit: 1,
      offset: 0,
    }));
    discardImportSessionMock.mockImplementation(async () => {
      running = null;
    });
    /** The sidebar's Import badge, as `LeftPanel` reads it. */
    function ImportBadge() {
      return <span data-testid="import-badge">{String(useImportAttention(true))}</span>;
    }
    renderWithProviders(
      <>
        <ImportBadge />
        <ImportScreen />
      </>,
    );

    await screen.findByTestId("resume-panel");
    await waitFor(() => expect(screen.getByTestId("import-badge")).toHaveTextContent("waiting"));
    await user.click(screen.getByText("discard-action"));

    expect(await screen.findByTestId("import-form")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByTestId("import-badge")).toHaveTextContent("null"));
  });

  it("also deletes the staging folder when discarding a this-device session", async () => {
    // W7: cancelRun already deletes the staging folder when a review is
    // cancelled (decision 16) -- a panel discard is the same operation reached
    // through a different button, and used to only call
    // discardImportSession, orphaning a potentially multi-GB folder.
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(
      session({
        stage: "upload",
        device_id: "this-device",
        staging_dir: "/home/u/message-crate/staging-260830",
      }),
    );
    renderWithProviders(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    await user.click(screen.getByText("discard-action"));

    expect(discardImportSessionMock).toHaveBeenCalledWith(7);
    expect(invokeDeleteStagingMock).toHaveBeenCalledWith({
      staging_dir: "/home/u/message-crate/staging-260830",
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
        stage: "upload",
        device_id: "another-device",
        staging_dir: "/home/u/message-crate/staging-260830",
      }),
    );
    renderWithProviders(<ImportScreen />);

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
        stage: "upload",
        staging_dir: "/home/u/message-crate/staging-260830",
        form: {
          source: "imessage-ios",
          backupPath: "/backups/iphone.tar",
          attachmentMedia: "copy",
          maxResolution: "720p",
          maxFps: "30",
          minSizeMb: "20",
          ownerPhones: [],
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
          assetMaxBytes: 512 * 1024 * 1024,
        },
      }),
    );
    renderWithProviders(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    await user.click(screen.getByText("resume-action"));

    expect(startImportMock).toHaveBeenCalledTimes(1);
    const [form, resume] = startImportMock.mock.calls[0] as [unknown, unknown];
    expect(form).toMatchObject({ source: "imessage-ios", backupPath: "/backups/iphone.tar" });
    expect(resume).toEqual({ sessionId: 7, stagingDir: "/home/u/message-crate/staging-260830" });
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
    assetMaxBytes: 512 * 1024 * 1024,
  };

  it.each([
    ["staging_review", "resume_review"],
    ["media_review", "resume_review"],
    ["media", "resume_media"],
  ] as const)(
    "routes a session at %s through resumeAtReview, not startImport or discard",
    async (stage, kind) => {
      const user = userEvent.setup();
      getActiveImportSessionMock.mockResolvedValue(
        session({
          stage,
          staging_dir: "/home/u/message-crate/staging-260830",
          form: restorableForm,
        }),
      );
      renderWithProviders(<ImportScreen />);

      await screen.findByTestId("resume-panel");
      expect(screen.getByTestId("resume-kind")).toHaveTextContent(kind);
      await user.click(screen.getByText("resume-action"));

      expect(resumeAtReviewMock).toHaveBeenCalledTimes(1);
      const [resumedSession, resumedForm] = resumeAtReviewMock.mock.calls[0] as [
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
    // failed recompute (useImportJob's resumeAtReview) never completes or
    // discards it -- it returns to the form phase instead. That phase
    // transition is what re-triggers this screen's own active-session
    // check, and since nothing was touched server-side, it finds the exact
    // same session and shows the panel again -- this is the retry.
    getActiveImportSessionMock.mockResolvedValue(
      session({ stage: "staging_review", form: restorableForm }),
    );
    const { rerender } = renderWithProviders(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("resume_review");
    expect(getActiveImportSessionMock).toHaveBeenCalledTimes(1);

    // Simulate resumeAtReview's failure path from inside the (mocked) hook:
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
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("resume_review");
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
          assetMaxBytes: 512 * 1024 * 1024,
        },
      }),
    );
    renderWithProviders(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("restart");
    await user.click(screen.getByText("resume-action"));

    expect(discardImportSessionMock).toHaveBeenCalledWith(7);
    // The old folder goes with the session: a restart writes into a new one,
    // and nothing will ever reach this one again.
    expect(invokeDeleteStagingMock).toHaveBeenCalledWith({
      staging_dir: "/home/u/message-crate/staging-260830",
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
          assetMaxBytes: 512 * 1024 * 1024,
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
    renderWithProviders(<ImportScreen />);

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
      stagingDir: "/home/u/message-crate/staging-260830",
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
    renderWithProviders(<ImportScreen />);

    expect(await screen.findByTestId("resume-panel")).toBeInTheDocument();
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("source_changed");
  });

  it("re-checks for an open session when the screen returns to the form", async () => {
    // A swallowed final /complete, or a restart whose discard failed,
    // leaves a session open server-side that the screen has forgotten. If
    // Back never re-checks, the user gets a form whose Import button 409s.
    getActiveImportSessionMock.mockResolvedValue(null);
    const { rerender } = renderWithProviders(<ImportScreen />);

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

    getActiveImportSessionMock.mockResolvedValue(session({ stage: "upload" }));
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
          assetMaxBytes: 512 * 1024 * 1024,
        },
      }),
    );
    // The panel stays mounted across this round trip by design, so the
    // second click lands on a live button.
    const pendingDiscard = deferred<void>();
    discardImportSessionMock.mockReturnValue(pendingDiscard.promise);
    renderWithProviders(<ImportScreen />);

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
      session({ stage: "upload", form: { nonsense: true } }),
    );
    renderWithProviders(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("resume_push");
    await user.click(screen.getByText("resume-action"));

    expect(startImportMock).not.toHaveBeenCalled();
    expect(await screen.findByTestId("resume-kind")).toHaveTextContent("settings_unreadable");
  });

  it("still drops to the form when discarding from the panel fails server-side", async () => {
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(session({ stage: "upload" }));
    discardImportSessionMock.mockRejectedValue(new Error("network down"));
    renderWithProviders(<ImportScreen />);

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
          assetMaxBytes: 512 * 1024 * 1024,
        },
      }),
    );
    discardImportSessionMock.mockRejectedValue(new Error("network down"));
    renderWithProviders(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    await user.click(screen.getByText("resume-action"));

    expect(discardImportSessionMock).toHaveBeenCalledWith(7);
    expect(startImportMock).toHaveBeenCalledTimes(1);
    const [form, resume] = startImportMock.mock.calls[0] as [unknown, unknown];
    expect(form).toMatchObject({ source: "imessage-ios", backupPath: "/backups/iphone.tar" });
    expect(resume).toBeUndefined();
  });

  it("asks for the backup password again when a resumed Staging will read an encrypted backup", async () => {
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(
      session({ stage: "write", form: storedForm({ backupPasswordGiven: true }) }),
    );
    renderWithProviders(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("resume_write");
    expect(screen.getByTestId("resume-secret")).toHaveTextContent("backupPassword");

    await user.click(screen.getByText("resume-action"));

    expect(startImportMock).toHaveBeenCalledTimes(1);
    const [form, resume, resumeWrite] = startImportMock.mock.calls[0] as [
      unknown,
      unknown,
      unknown,
    ];
    expect(form).toMatchObject({ backupPassword: "typed-secret", whatsappKey: "" });
    expect(resume).toBeUndefined();
    expect(resumeWrite).toMatchObject({ sessionId: 7 });
  });

  it("asks for the backup password again when a restart will read an encrypted backup", async () => {
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(
      session({ stage: "parse", form: storedForm({ backupPasswordGiven: true }) }),
    );
    renderWithProviders(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("restart");
    expect(screen.getByTestId("resume-secret")).toHaveTextContent("backupPassword");

    await user.click(screen.getByText("resume-action"));

    expect(discardImportSessionMock).toHaveBeenCalledWith(7);
    expect(startImportMock).toHaveBeenCalledTimes(1);
    const [form, resume, resumeWrite] = startImportMock.mock.calls[0] as [
      unknown,
      unknown,
      unknown,
    ];
    expect(form).toMatchObject({ backupPassword: "typed-secret", whatsappKey: "" });
    expect(resume).toBeUndefined();
    expect(resumeWrite).toBeUndefined();
  });

  it.each([
    ["a resumed Staging", "write", "resume_write"],
    ["a restart", "parse", "restart"],
  ] as const)(
    "asks for the WhatsApp key again when %s will read the backup",
    async (_label, stage, kind) => {
      const user = userEvent.setup();
      getActiveImportSessionMock.mockResolvedValue(
        session({
          source: "whatsapp",
          stage,
          form: storedForm({
            source: "whatsapp-android",
            backupPath: "/backups/whatsapp",
            whatsappKeyGiven: true,
          }),
        }),
      );
      renderWithProviders(<ImportScreen />);

      await screen.findByTestId("resume-panel");
      expect(screen.getByTestId("resume-kind")).toHaveTextContent(kind);
      expect(screen.getByTestId("resume-secret")).toHaveTextContent("whatsappKey");

      await user.click(screen.getByText("resume-action"));

      expect(startImportMock).toHaveBeenCalledTimes(1);
      const [form] = startImportMock.mock.calls[0] as [unknown];
      expect(form).toMatchObject({ whatsappKey: "typed-secret", backupPassword: "" });
    },
  );

  it("asks for nothing when the stored Import Run had no password or key", async () => {
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(session({ stage: "write", form: storedForm() }));
    renderWithProviders(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("resume_write");
    expect(screen.getByTestId("resume-secret")).toHaveTextContent("none");

    await user.click(screen.getByText("resume-action"));

    const [form] = startImportMock.mock.calls[0] as [unknown];
    expect(form).toMatchObject({ backupPassword: "", whatsappKey: "" });
  });

  it("asks for nothing on a resume into Upload, which reads no backup", async () => {
    const user = userEvent.setup();
    getActiveImportSessionMock.mockResolvedValue(
      session({ stage: "upload", form: storedForm({ backupPasswordGiven: true }) }),
    );
    renderWithProviders(<ImportScreen />);

    await screen.findByTestId("resume-panel");
    expect(screen.getByTestId("resume-kind")).toHaveTextContent("resume_push");
    expect(screen.getByTestId("resume-secret")).toHaveTextContent("none");

    await user.click(screen.getByText("resume-action"));

    expect(startImportMock).toHaveBeenCalledTimes(1);
    const [form, resume] = startImportMock.mock.calls[0] as [unknown, unknown];
    expect(form).toMatchObject({ backupPassword: "", whatsappKey: "" });
    expect(resume).toMatchObject({ sessionId: 7 });
  });
});

describe("ImportScreen gates", () => {
  beforeEach(() => {
    hookState.phase = "form";
    hookState.stagingSummary = null;
    hookState.mediaSummary = null;
    hookState.mediaFailedCount = null;
    hookState.mediaToolsMissing = false;
    hookState.mediaPartiallyRan = false;
    hookState.resumeError = null;
    hookState.sourceIdentities = null;
    startImportMock.mockReset();
    resumeAtReviewMock.mockReset();
    resumeAtReviewMock.mockResolvedValue(undefined);
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

  it("shows the Staging Review inside the run, with approve and cancel wired to the hook", async () => {
    hookState.phase = "staging_review";
    hookState.stagingSummary = stagingSummary({ contactIdentifiers: ["+15551234567"] });
    hookState.sourceIdentities = ["+15550001111"];
    const user = userEvent.setup();
    renderWithProviders(<ImportScreen />);

    expect(await screen.findByTestId("import-run")).toBeInTheDocument();
    expect(screen.queryByTestId("import-form")).not.toBeInTheDocument();
    expect(screen.getByTestId("run-review-waiting")).toHaveTextContent("staging");
    expect(screen.getByTestId("run-has-identities")).toHaveTextContent("true");

    await user.click(screen.getByText("run-approve"));
    expect(approveMock).toHaveBeenCalledTimes(1);

    await user.click(screen.getByText("run-cancel-run"));
    expect(cancelRunMock).toHaveBeenCalledTimes(1);
  });

  it("shows the Media Review inside the run, without the backup's identities", async () => {
    hookState.phase = "media_review";
    hookState.stagingSummary = stagingSummary();
    hookState.mediaSummary = stagingSummary();
    hookState.sourceIdentities = ["+15550001111"];
    const user = userEvent.setup();
    renderWithProviders(<ImportScreen />);

    expect(await screen.findByTestId("import-run")).toBeInTheDocument();
    expect(screen.getByTestId("run-review-waiting")).toHaveTextContent("media");
    expect(screen.getByTestId("run-has-identities")).toHaveTextContent("false");

    await user.click(screen.getByText("run-approve"));
    expect(approveMock).toHaveBeenCalledTimes(1);

    await user.click(screen.getByText("run-cancel-run"));
    expect(cancelRunMock).toHaveBeenCalledTimes(1);
  });

  it("returns to the form when the person goes back from a finished run", async () => {
    hookState.phase = "done";
    const user = userEvent.setup();
    renderWithProviders(<ImportScreen />);

    await user.click(await screen.findByText("run-back"));
    expect(returnToFormMock).toHaveBeenCalledTimes(1);
  });

  it("looks up which of the staged contacts are unknown, in one batch under the server cap", async () => {
    hookState.phase = "staging_review";
    hookState.stagingSummary = stagingSummary({ contactIdentifiers: ["a", "b", "c"] });
    apiPostMock.mockResolvedValue({ items: ["a", "c"], total: 2, limit: 500, offset: 0 });
    renderWithProviders(<ImportScreen />);

    await screen.findByTestId("import-run");
    await act(async () => {
      await Promise.resolve();
    });

    expect(apiPostMock).toHaveBeenCalledTimes(1);
    expect(apiPostMock).toHaveBeenCalledWith(
      { identifiers: ["a", "b", "c"] },
      { signal: expect.any(AbortSignal) },
    );
    await waitFor(() => expect(screen.getByTestId("run-unknown-contacts")).toHaveTextContent("2"));
    expect(screen.getByTestId("run-unknown-contacts-error")).toHaveTextContent("null");
  });

  it("batches the contact-match lookup at 500 identifiers per request and sums unknown across batches", async () => {
    hookState.phase = "staging_review";
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
    renderWithProviders(<ImportScreen />);

    await screen.findByTestId("import-run");
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(apiPostMock).toHaveBeenCalledTimes(2);
    const bodies = apiPostMock.mock.calls.map(([body]) => body as { identifiers: string[] });
    expect(bodies[0]?.identifiers).toHaveLength(500);
    expect(bodies[1]?.identifiers).toHaveLength(120);
    await waitFor(() =>
      expect(screen.getByTestId("run-unknown-contacts")).toHaveTextContent("430"),
    );
  });

  it("shows why the unknown-contact count is missing when the lookup fails", async () => {
    hookState.phase = "staging_review";
    hookState.stagingSummary = stagingSummary({ contactIdentifiers: ["a"] });
    apiPostMock.mockRejectedValue(new Error("network down"));
    renderWithProviders(<ImportScreen />);

    await screen.findByTestId("import-run");
    await waitFor(() =>
      expect(screen.getByTestId("run-unknown-contacts-error")).toHaveTextContent("network down"),
    );
    expect(screen.getByTestId("run-unknown-contacts")).toHaveTextContent("null");
  });

  it("shows the identity stop screen for the identity_stop phase", async () => {
    hookState.phase = "identity_stop";
    hookState.sourceIdentities = ["+15550001111"];
    renderWithProviders(<ImportScreen />);

    expect(
      await screen.findByText("None of the addresses this backup sent from are on your profile."),
    ).toBeInTheDocument();
  });

  it("shows a factual line when adding an identity to the profile fails", async () => {
    hookState.phase = "identity_stop";
    hookState.sourceIdentities = ["+15550001111"];
    apiPostMock.mockRejectedValue(new Error("network down"));
    const user = userEvent.setup();
    renderWithProviders(<ImportScreen />);

    await user.click(await screen.findByText("Add to profile"));

    expect(await screen.findByText("The server didn't add that address.")).toBeInTheDocument();
  });
});

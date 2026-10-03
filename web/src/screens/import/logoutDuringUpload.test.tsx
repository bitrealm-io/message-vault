/** @vitest-environment jsdom */

// Logging out while an Upload runs (#1155). The push holds the session token
// it started with, so revoking the session under it would have every later
// request refused and every remaining conversation recorded as failed. Logout
// asks first, then pauses the Upload, waits for the push to halt and the pause
// to be recorded, and only then revokes the session.

import { act, renderHook, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getToken } from "../../lib/api";
import type { PushFinishedReport, TauriJobResult } from "../../lib/tauri";

/** Every call that matters to the order, in the order it was made. */
const calls: string[] = [];

const serverLogoutMock = vi.fn();
const completeImportMock = vi.fn();
const setImportStageMock = vi.fn();
const saveRunRecordMock = vi.fn();
const cancelMock = vi.fn();

/** Settles the running push with the report the desktop side sends. */
let finishPush: ((result: TauriJobResult) => void) | null = null;

vi.mock("../../lib/tauri", () => ({
  awaitTauriJob: async (_job: string, invokeFn: () => Promise<void>) => {
    await invokeFn();
    calls.push("push started");
    return new Promise<TauriJobResult>((resolve) => {
      finishPush = resolve;
    });
  },
  invokeCancel: async () => {
    calls.push("cancel");
    cancelMock();
  },
  invokePush: async () => {},
  invokeReadImportRunRecord: async () => null,
  invokeSaveImportRunRecord: async () => {
    calls.push("pause recorded");
    saveRunRecordMock();
  },
  invokeDeleteStaging: async () => {},
}));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  logout: async () => {
    calls.push("session revoked");
    serverLogoutMock();
  },
  getSession: async () => ({}),
  completeImport: (...args: unknown[]) => completeImportMock(...args),
}));

vi.mock("../../lib/importSession", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/importSession")>()),
  setImportStage: (...args: unknown[]) => setImportStageMock(...args),
}));

vi.mock("../../lib/useAccountProfile", () => ({
  fetchAccountProfileFor: async () => ({}),
  useFetchAccountProfile: () => async () => ({ phones: [], emails: [] }),
}));

vi.mock("../../lib/tauri-check", () => ({
  isTauri: () => true,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => {
    throw new Error("no window in tests");
  },
}));

const { AuthProvider, useAuth } = await import("../../lib/auth");
const { isUploadRunning } = await import("../../lib/runningUpload");
const { useImportJob, resetImportRun } = await import("./useImportJob");

const MIB = 1024 * 1024;

const form = {
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
  assetMaxBytes: 512 * MIB,
};

/** What `run.rs` reports when the cancel flag stops the push after 200 of 681. */
function pausedReport(): PushFinishedReport {
  return {
    ok: false,
    cancelled: true,
    messages: 4_000,
    messages_attempted: 4_000,
    messages_inserted: 4_000,
    messages_deduped: 0,
    messages_failed: 0,
    assets_uploaded: 0,
    assets_bytes: 0,
    conversations_ok: 200,
    conversations_total: 681,
    conversations_failed: 0,
    conversations_skipped: 0,
    conversations_cancelled: 481,
    results: [],
  };
}

type Screen = {
  job: ReturnType<typeof useImportJob>;
  auth: ReturnType<typeof useAuth>;
};

/** The Import screen's hook beside the login state, under the real AuthProvider. */
async function logIn(): Promise<{ current: Screen }> {
  const { result } = renderHook((): Screen => ({ job: useImportJob(), auth: useAuth() }), {
    wrapper: AuthProvider,
  });
  await act(() => result.current.auth.login("http://127.0.0.1:8080", "session-token", 7));
  return result;
}

/** Log in, and start a resumed Upload that runs until the test settles it. */
async function startUpload(): Promise<{ current: Screen }> {
  const result = await logIn();
  act(() => {
    void result.current.job.startImport(form, {
      sessionId: 42,
      stagingDir: "/home/sam/staging-iphone",
    });
  });
  await waitFor(() => expect(calls).toContain("push started"));
  return result;
}

/** What the account menu's Log out does: it does not wait for the logout. */
function pressLogOut(result: { current: Screen }): void {
  act(() => {
    void result.current.auth.logout();
  });
}

describe("logging out during an Upload", () => {
  beforeEach(() => {
    localStorage.clear();
    resetImportRun();
    calls.length = 0;
    finishPush = null;
    serverLogoutMock.mockReset();
    completeImportMock.mockReset();
    completeImportMock.mockResolvedValue({});
    setImportStageMock.mockReset();
    setImportStageMock.mockResolvedValue(undefined);
    saveRunRecordMock.mockReset();
    cancelMock.mockReset();
    // The push stops when the cancel flag is set, the way `run.rs` does.
    cancelMock.mockImplementation(() => finishPush?.({ summary: "Push", report: pausedReport() }));
  });

  // An Upload a test left running would make the next test's logout ask.
  afterEach(async () => {
    finishPush?.({ summary: "Push", report: pausedReport() });
    await waitFor(() => expect(isUploadRunning()).toBe(false));
  });

  it("asks first, pauses the Upload, and revokes the session only once the pause is recorded", async () => {
    const user = userEvent.setup();
    const result = await startUpload();

    pressLogOut(result);

    expect(
      await screen.findByText(
        "An Upload is running. Logging out pauses it; you can resume it after you log in.",
      ),
    ).toBeTruthy();
    // Nothing happens until the person chooses.
    expect(calls).not.toContain("cancel");
    expect(serverLogoutMock).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Log out" }));

    await waitFor(() => expect(serverLogoutMock).toHaveBeenCalled());
    expect(calls).toEqual(["push started", "cancel", "pause recorded", "session revoked"]);
    // The run stays at its Upload stage: it is paused, not completed, and no
    // conversation was recorded as failed.
    expect(completeImportMock).not.toHaveBeenCalled();
    expect(setImportStageMock.mock.calls.map((call) => call[1])).toEqual(["upload"]);
    expect(getToken()).toBeNull();
    expect(result.current.auth.isAuthenticated).toBe(false);
    // The paused run is the account's (#1085): logged out, the screen shows
    // the form, and the account finds the run again when it logs back in.
    expect(result.current.job.phase).toBe("form");
    await act(() => result.current.auth.login("http://127.0.0.1:8080", "next-session-token", 7));
    expect(result.current.job.summaryView?.status).toBe("paused");
    expect(result.current.job.summaryView?.filesFailed).toBe(0);
  });

  it("leaves the Upload running and the account logged in when the person goes back", async () => {
    const user = userEvent.setup();
    const result = await startUpload();

    pressLogOut(result);
    await user.click(await screen.findByRole("button", { name: "Go back" }));

    await waitFor(() =>
      expect(
        screen.queryByText(
          "An Upload is running. Logging out pauses it; you can resume it after you log in.",
        ),
      ).toBeNull(),
    );
    expect(cancelMock).not.toHaveBeenCalled();
    expect(serverLogoutMock).not.toHaveBeenCalled();
    expect(result.current.auth.isAuthenticated).toBe(true);
    expect(result.current.job.running).toBe(true);
  });

  it("logs out without asking when no Upload runs", async () => {
    const result = await logIn();

    pressLogOut(result);

    await waitFor(() => expect(serverLogoutMock).toHaveBeenCalled());
    expect(screen.queryByText(/An Upload is running/)).toBeNull();
    expect(cancelMock).not.toHaveBeenCalled();
  });

  it("pauses the Upload without asking when the account was just deleted", async () => {
    const result = await startUpload();

    await act(() => result.current.auth.logout({ ask: false }));

    expect(screen.queryByText(/An Upload is running/)).toBeNull();
    expect(calls).toEqual(["push started", "cancel", "pause recorded", "session revoked"]);
    expect(completeImportMock).not.toHaveBeenCalled();
  });
});

/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ImportSummaryView } from "../../components/import/ImportSummaryPanel";
import type { StagingSummary } from "../../lib/tauri";
import { VaultProviders } from "../../test/vaultProviders";
import ImportRunView from "./ImportRunView";
import { type ImportStep, stepsFor } from "./importProgressState";
import { attachmentsAsked, runHeading, sourceDisplayName } from "./importRunCopy";
import type { ImportJobFormValues } from "./useImportJob";

const openPathInExplorer = vi.fn();
const getImportMock = vi.fn();
const getImportContactsMock = vi.fn();
const navigateMock = vi.fn();

vi.mock("../../lib/openPath", () => ({
  openPathInExplorer: (...args: unknown[]) => openPathInExplorer(...args),
}));

vi.mock("../../lib/auth", () => ({
  useAuth: () => ({ accountId: 7, token: "test-token", isAuthenticated: true }),
}));

vi.mock("../../lib/vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/vaultApi")>()),
  getImport: (...args: unknown[]) => getImportMock(...args),
  getImportContacts: (...args: unknown[]) => getImportContactsMock(...args),
}));

vi.mock("react-router-dom", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-router-dom")>()),
  useNavigate: () => navigateMock,
}));

function form(overrides: Partial<ImportJobFormValues> = {}): ImportJobFormValues {
  return {
    source: "imessage-ios",
    backupPath: "/backups/iphone",
    backupPassword: "",
    attachmentMedia: "convert",
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
    whatsappKey: "",
    whatsappWa: "",
    whatsappMedia: "",
    whatsappDb: "",
    whatsappBusiness: false,
    ...overrides,
  };
}

function staged(overrides: Partial<StagingSummary> = {}): StagingSummary {
  return {
    conversations: 312,
    messages: 48205,
    contactIdentifiers: [],
    attachments: 6118,
    attachmentBytes: 9.4 * 1024 * 1024 * 1024,
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

function finished(overrides: Partial<ImportSummaryView> = {}): ImportSummaryView {
  return {
    status: "completed",
    messagesParsed: 48205,
    messagesAttempted: 48205,
    messagesInserted: 47910,
    messagesDeduped: 295,
    messagesFailed: 0,
    durationMs: 660_000,
    issues: [],
    ...overrides,
  };
}

function renderView(props: Partial<Parameters<typeof ImportRunView>[0]> = {}) {
  const steps: ImportStep[] = stepsFor("convert");
  return render(
    <VaultProviders>
      <MemoryRouter>
        <ImportRunView
          phase="running"
          steps={steps}
          running
          form={form()}
          stagingSummary={null}
          mediaDelta={null}
          summaryView={null}
          stagingDir={null}
          importSessionId={null}
          approvalWaiting={null}
          onCancel={() => {}}
          onReview={() => {}}
          onImportAnother={() => {}}
          {...props}
        />
      </MemoryRouter>
    </VaultProviders>,
  );
}

describe("runHeading and the asked-for lines", () => {
  it("names the source while the run is going", () => {
    expect(runHeading("running", form(), null, undefined)).toBe(
      "Importing from iMessage · iPhone backup",
    );
    expect(sourceDisplayName("whatsapp-android")).toBe("WhatsApp · Android");
    expect(sourceDisplayName("sms-backup-restore")).toBe("SMS Backup & Restore");
  });

  it("leads with what was imported once the run is done", () => {
    expect(runHeading("done", form(), finished(), "Import complete")).toBe(
      "Imported 47,910 messages",
    );
    expect(runHeading("done", form(), finished({ status: "failed" }), "Import failed")).toBe(
      "Import failed",
    );
  });

  it("states the media settings only when they apply", () => {
    expect(attachmentsAsked(form({ attachmentMedia: "copy" }))).toBe("Copy");
    expect(attachmentsAsked(form())).toBe("Convert · up to 720p, 30 fps, files over 20 MB");
  });
});

describe("ImportRunView", () => {
  beforeEach(() => {
    openPathInExplorer.mockReset();
    openPathInExplorer.mockResolvedValue(undefined);
    getImportMock.mockReset();
    getImportContactsMock.mockReset();
    getImportContactsMock.mockResolvedValue({
      items: [{ id: 1, name: "Ada Lovelace", reason: "created" }],
      total: 1,
      limit: 40,
      offset: 0,
    });
    navigateMock.mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it("shows what was asked for, with the staging folder and log links", async () => {
    const user = userEvent.setup();
    const staging = "/home/sam/message-vault/staging-iphone";
    renderView({ stagingDir: staging });

    expect(screen.getByRole("heading", { name: "Importing from iMessage · iPhone backup" }));
    expect(screen.getByText("What you asked for")).toBeInTheDocument();
    expect(screen.getByText("/backups/iphone")).toBeInTheDocument();
    expect(screen.getByText("Convert · up to 720p, 30 fps, files over 20 MB")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: staging }));
    expect(openPathInExplorer).toHaveBeenCalledWith(staging);
    await user.click(screen.getByRole("button", { name: "vault-push.log" }));
    expect(openPathInExplorer).toHaveBeenCalledWith(`${staging}/vault-push.log`);
  });

  it("offers Cancel while a stage runs, disabled while a not-cancellable step runs", () => {
    renderView({ cancelDisabled: true });
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
  });

  it("adds the Staging result once it is in, and shows the approval waiting", async () => {
    const onReview = vi.fn();
    const user = userEvent.setup();
    renderView({
      phase: "staging_approval",
      running: false,
      stagingSummary: staged(),
      approvalWaiting: "staging",
      onReview,
    });

    expect(screen.getByText("312")).toBeInTheDocument();
    expect(screen.getByText("48,205")).toBeInTheDocument();
    expect(screen.getByText(/6,118 · 9\.4 GB/)).toBeInTheDocument();
    expect(screen.getByText(/Staging Approval · waiting for/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Review and approve" }));
    expect(onReview).toHaveBeenCalledTimes(1);
  });

  it("adds the Media result underneath the Staging one", () => {
    renderView({
      phase: "media_approval",
      running: false,
      stagingSummary: staged(),
      mediaDelta: { lostCount: 3, stillFlagged: [], cameOutFine: 0, hasChanges: true },
      approvalWaiting: "media",
    });
    expect(screen.getByText("3 files will not be uploaded.")).toBeInTheDocument();
    expect(screen.getByText(/Media Approval · waiting for/)).toBeInTheDocument();
  });

  it("leads a finished run with where to go next", async () => {
    getImportMock.mockResolvedValue({
      id: 42,
      source: "imessage",
      started_at: "2026-09-09T10:00:00Z",
      finished_at: "2026-09-09T10:11:00Z",
      contacts_new: 37,
      contacts_changed: 16,
    });
    const onImportAnother = vi.fn();
    const user = userEvent.setup();
    renderView({
      phase: "done",
      running: false,
      stagingSummary: staged(),
      summaryView: finished(),
      importSessionId: 42,
      completionText: "Import complete",
      onImportAnother,
    });

    expect(screen.getByRole("heading", { name: "Imported 47,910 messages" })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText("37 new, 16 changed")).toBeInTheDocument());

    await user.click(screen.getByRole("button", { name: "Conversations this import added" }));
    expect(navigateMock).toHaveBeenCalledWith("/?q=import%3A%2342");

    await user.click(screen.getByRole("button", { name: "Contacts it touched" }));
    expect(navigateMock).toHaveBeenCalledWith("/group/imessage-import-2026-09-09");

    await user.click(screen.getByRole("button", { name: "Import another" }));
    expect(onImportAnother).toHaveBeenCalledTimes(1);
  });

  it("offers only Import another after a failed run", () => {
    renderView({
      phase: "done",
      running: false,
      summaryView: finished({ status: "failed" }),
      importSessionId: 42,
      completionText: "Import failed",
    });
    expect(screen.getByRole("heading", { name: "Import failed" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Import another" })).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Conversations this import added" }),
    ).not.toBeInTheDocument();
    expect(getImportMock).not.toHaveBeenCalled();
  });

  it("keeps Import Errors heading and table when issues exist", () => {
    renderView({
      phase: "done",
      running: false,
      summaryView: finished({
        status: "completed_with_issues",
        issues: [{ kind: "warn", step: "upload", item: "chat.jsonl", reason: "Skipped one" }],
      }),
      importSessionId: null,
      completionText: "Import completed with issues",
    });
    expect(screen.getByRole("heading", { name: "Import Errors" })).toBeInTheDocument();
  });
});

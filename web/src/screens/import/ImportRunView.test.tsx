/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ImportSummaryView } from "../../components/import/ImportSummaryPanel";
import type { AttachmentForecast, StagingSummary } from "../../lib/tauri";
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
    ownerHandles: [],
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
    assetMaxBytes: 50 * 1024 * 1024,
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

const MB = 1024 * 1024;

function file(
  name: string,
  sizeMb: number,
  verdict: AttachmentForecast["verdict"],
  estimateMb = sizeMb,
): AttachmentForecast {
  return {
    path: `attachments/${name}`,
    name,
    sizeBytes: sizeMb * MB,
    estimateBytes: estimateMb * MB,
    verdict,
  };
}

function stepsAt(
  mode: "convert" | "copy",
  statuses: Partial<Record<string, ImportStep["status"]>>,
): ImportStep[] {
  return stepsFor(mode).map((step) => ({ ...step, status: statuses[step.label] ?? "pending" }));
}

function renderView(props: Partial<Parameters<typeof ImportRunView>[0]> = {}) {
  return render(
    <VaultProviders>
      <MemoryRouter>
        <ImportRunView
          phase="running"
          steps={stepsAt("convert", { Staging: "active" })}
          running
          form={form()}
          stagingSummary={null}
          mediaSummary={null}
          mediaFailedCount={null}
          summaryView={null}
          stagingDir={null}
          importSessionId={null}
          reviewWaiting={null}
          unknownContacts={null}
          onApprove={() => {}}
          onCancelRun={() => {}}
          onCancel={() => {}}
          onBack={() => {}}
          {...props}
        />
      </MemoryRouter>
    </VaultProviders>,
  );
}

/** The list item for one row of the stage list, found by its exact label. */
function stageRow(label: string): HTMLElement {
  const item = screen.getByText(label, { selector: "li > div > div > span" }).closest("li");
  if (!item) throw new Error(`no stage row for ${label}`);
  return item;
}

const WAITING_STAGING = "Staging Review";
const WAITING_MEDIA = "Media Review";

describe("runHeading and the operation line", () => {
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
    expect(
      runHeading("done", form(), finished({ status: "completed_with_issues" }), "with issues"),
    ).toBe("Imported 47,910 messages, with errors");
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

  it("lists every stage and review of the run, in order", () => {
    renderView();
    const labels = screen
      .getAllByRole("listitem")
      .map((item) => item.querySelector("div > div > span")?.textContent);
    expect(labels).toEqual(["Staging", "Staging Review", "Media", "Media Review", "Upload"]);
  });

  it("has no Media rows when the operation has no Media stage", () => {
    renderView({
      form: form({ attachmentMedia: "copy" }),
      steps: stepsAt("copy", { Staging: "active" }),
    });
    const labels = screen
      .getAllByRole("listitem")
      .map((item) => item.querySelector("div > div > span")?.textContent);
    expect(labels).toEqual(["Staging", "Staging Review", "Upload"]);
  });

  it("puts the backup path under the heading and the staging directory in the Staging row", async () => {
    const user = userEvent.setup();
    const staging = "/home/sam/message-vault/staging-iphone";
    renderView({ stagingDir: staging });

    expect(screen.getByRole("heading", { name: "Importing from iMessage · iPhone backup" }));
    expect(screen.getByText("/backups/iphone")).toBeInTheDocument();
    expect(screen.queryByText("What you asked for")).not.toBeInTheDocument();

    const row = within(stageRow("Staging"));
    expect(row.getByText("Convert · up to 720p, 30 fps, files over 20 MB")).toBeInTheDocument();
    await user.click(row.getByRole("button", { name: staging }));
    expect(openPathInExplorer).toHaveBeenCalledWith(staging);
  });

  it("offers no import log until Upload has started, then shows it in the Upload row", async () => {
    const user = userEvent.setup();
    const staging = "/home/sam/message-vault/staging-iphone";
    const view = renderView({ stagingDir: staging });
    expect(screen.queryByRole("button", { name: "vault-push.log" })).not.toBeInTheDocument();

    view.unmount();
    renderView({
      stagingDir: staging,
      steps: stepsAt("convert", { Staging: "done", Media: "done", Upload: "active" }),
    });
    await user.click(within(stageRow("Upload")).getByRole("button", { name: "vault-push.log" }));
    expect(openPathInExplorer).toHaveBeenCalledWith(`${staging}/vault-push.log`);
  });

  it("shows the options group only when a switch is on", () => {
    const view = renderView();
    expect(screen.queryByText("Options")).not.toBeInTheDocument();
    view.unmount();
    renderView({ form: form({ obfuscate: true }) });
    expect(screen.getByText("Options")).toBeInTheDocument();
    expect(screen.getByText("Obfuscate")).toBeInTheDocument();
    expect(screen.queryByText("Force reprocessing")).not.toBeInTheDocument();
  });

  it("offers Cancel inside the running stage, disabled while a not-cancellable step runs", () => {
    renderView({ cancelDisabled: true });
    expect(within(stageRow("Staging")).getByRole("button", { name: "Cancel" })).toBeDisabled();
  });

  it("keeps Cancel on screen while running with no stage active", () => {
    renderView({ steps: stepsAt("convert", { Staging: "done" }), cancelDisabled: true });
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
  });

  it("waits at the Staging Review with the staged facts, the limit and the decision", async () => {
    const onApprove = vi.fn();
    const onCancelRun = vi.fn();
    const user = userEvent.setup();
    renderView({
      phase: "staging_review",
      running: false,
      form: form({ attachmentMedia: "copy" }),
      steps: stepsAt("copy", { Staging: "done" }),
      stagingSummary: staged({
        contactIdentifiers: ["+15550100", "+15550101", "a@example.com"],
        forecasts: [
          file("small.mov", 60, "probably_too_big"),
          file("big.mov", 212, "probably_too_big"),
        ],
      }),
      reviewWaiting: "staging",
      unknownContacts: 1,
      onApprove,
      onCancelRun,
    });

    const staging = within(stageRow("Staging"));
    expect(staging.getByText("312")).toBeInTheDocument();
    expect(staging.getByText("48,205")).toBeInTheDocument();
    expect(staging.getByText("6,118")).toBeInTheDocument();
    expect(staging.getByText("9.4 GB")).toBeInTheDocument();

    const review = within(stageRow(WAITING_STAGING));
    expect(review.getByText("Existing").nextSibling).toHaveTextContent("2");
    expect(review.getByText("New").nextSibling).toHaveTextContent("1");
    expect(review.getByText("Size limit per file").nextSibling).toHaveTextContent("50 MB");
    expect(review.queryByText(/estimates/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument();

    // The files are listed only once the count is opened, largest first.
    expect(review.queryByText("big.mov")).not.toBeInTheDocument();
    await user.click(review.getByRole("button", { name: /Files over the limit/ }));
    const names = review.getAllByText(/\.mov$/).map((node) => node.textContent);
    expect(names).toEqual(["big.mov", "small.mov"]);
    // What becomes of them is said beside the label, not in a sentence above the list.
    expect(review.getByRole("button", { name: /Files over the limit/ })).toHaveTextContent(
      "Skip vault upload",
    );
    expect(review.queryByText(/stay out of the vault/)).not.toBeInTheDocument();
    expect(review.getByText("Awaiting approval")).toBeInTheDocument();

    await user.click(review.getByRole("button", { name: "Upload to vault" }));
    expect(onApprove).toHaveBeenCalledTimes(1);
    await user.click(review.getByRole("button", { name: "Cancel this import" }));
    expect(onCancelRun).toHaveBeenCalledTimes(1);
  });

  it("leaves the contact split out when the lookup has no answer", () => {
    renderView({
      phase: "staging_review",
      running: false,
      steps: stepsAt("convert", { Staging: "done" }),
      stagingSummary: staged({ contactIdentifiers: ["+15550100"] }),
      reviewWaiting: "staging",
      unknownContacts: null,
    });
    expect(screen.getByText("Contacts")).toBeInTheDocument();
    expect(screen.queryByText("Existing")).not.toBeInTheDocument();
  });

  it("sorts the estimates into three piles when a Media stage is coming", async () => {
    const user = userEvent.setup();
    renderView({
      phase: "staging_review",
      running: false,
      form: form({ attachmentMedia: "compress" }),
      steps: stepsAt("convert", { Staging: "done" }),
      stagingSummary: staged({
        forecasts: [
          file("fits.mov", 96, "likely_fits", 38),
          file("huge.mov", 212, "probably_too_big", 84),
          file("grows.mov", 46, "may_grow", 52),
          file("scan.tiff", 71, "cannot_process"),
        ],
      }),
      reviewWaiting: "staging",
    });
    const review = within(stageRow(WAITING_STAGING));
    expect(review.getByText("Compression estimates")).toBeInTheDocument();
    expect(review.getByText("Media has not run yet")).toBeInTheDocument();
    expect(review.getByRole("button", { name: /Likely within limit/ })).toHaveTextContent("1");
    expect(review.getByRole("button", { name: /Not audio or video/ })).toHaveTextContent("1");

    // A file that may grow past the limit sits with the ones expected to stay over it.
    const mayExceed = review.getByRole("button", { name: /May exceed limit/ });
    expect(mayExceed).toHaveTextContent("2");
    await user.click(mayExceed);
    expect(review.getByText("212 MB → 84 MB")).toBeInTheDocument();
    expect(review.getByText("46 MB → 52 MB")).toBeInTheDocument();
    expect(review.getByRole("button", { name: "Compress media" })).toBeEnabled();
  });

  it("blocks approving when the Media tools are missing", () => {
    renderView({
      phase: "staging_review",
      running: false,
      steps: stepsAt("convert", { Staging: "done" }),
      stagingSummary: staged(),
      reviewWaiting: "staging",
      mediaToolsMissing: true,
    });
    expect(screen.getByRole("button", { name: "Convert media" })).toBeDisabled();
    expect(screen.getByText(/Media needs ffmpeg/)).toBeInTheDocument();
  });

  it("drops the estimates when Media already ran partway", () => {
    renderView({
      phase: "staging_review",
      running: false,
      steps: stepsAt("convert", { Staging: "done" }),
      stagingSummary: staged({ forecasts: [file("fits.mov", 96, "likely_fits", 38)] }),
      reviewWaiting: "staging",
      mediaPartiallyRan: true,
    });
    expect(screen.queryByText("Conversion estimates")).not.toBeInTheDocument();
    expect(screen.getByText(/picks up where it left off/)).toBeInTheDocument();
  });

  it("waits at the Media Review with what is true now, and no comparison", () => {
    renderView({
      phase: "media_review",
      running: false,
      steps: stepsAt("convert", { Staging: "done", Media: "done" }),
      stagingSummary: staged(),
      mediaSummary: staged({
        attachmentBytes: 4.1 * 1024 * 1024 * 1024,
        forecasts: [file("huge-mv.mp4", 84, "probably_too_big")],
      }),
      mediaFailedCount: 2,
      reviewWaiting: "media",
    });

    expect(within(stageRow("Staging Review")).getByText("Approved")).toBeInTheDocument();
    // The Staging row keeps what Staging made; Media's row holds what Media made.
    expect(within(stageRow("Staging")).getByText("9.4 GB")).toBeInTheDocument();
    const media = within(stageRow("Media"));
    expect(media.getByText("4.1 GB")).toBeInTheDocument();
    expect(media.getByText("Could not be converted").nextSibling).toHaveTextContent("2");

    const review = within(stageRow(WAITING_MEDIA));
    expect(review.getByRole("button", { name: /Files over the limit/ })).toHaveTextContent("1");
    expect(review.getByRole("button", { name: "Upload to vault" })).toBeInTheDocument();
    expect(screen.queryByText(/since you approved/)).not.toBeInTheDocument();
  });

  it("leads a finished run with where to go next, and Back returns to the form", async () => {
    getImportMock.mockResolvedValue({
      id: 42,
      source: "imessage",
      started_at: "2026-09-09T10:00:00Z",
      finished_at: "2026-09-09T10:11:00Z",
      contacts_new: 37,
      contacts_changed: 16,
    });
    const onBack = vi.fn();
    const user = userEvent.setup();
    renderView({
      phase: "done",
      running: false,
      steps: stepsAt("convert", { Staging: "done", Media: "done", Upload: "done" }),
      stagingSummary: staged(),
      summaryView: finished({ attachmentsUploaded: 6104 }),
      importSessionId: 42,
      completionText: "Import complete",
      onBack,
    });

    expect(screen.getByRole("heading", { name: "Imported 47,910 messages" })).toBeInTheDocument();
    expect(screen.getByText("iMessage · iPhone backup · /backups/iphone")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Import another" })).not.toBeInTheDocument();

    const upload = within(stageRow("Upload"));
    expect(upload.getByText("Duplicate").nextSibling).toHaveTextContent("295");
    expect(upload.getByText("Uploaded").nextSibling).toHaveTextContent("6,104");
    await waitFor(() => expect(upload.getByText("Modified").nextSibling).toHaveTextContent("16"));

    await user.click(upload.getByRole("button", { name: /Contact list/ }));
    await waitFor(() => expect(upload.getByText("Ada Lovelace")).toBeInTheDocument());

    await user.click(screen.getByRole("button", { name: "View imported conversations" }));
    expect(navigateMock).toHaveBeenCalledWith("/?q=import%3A%2342");
    await user.click(screen.getByRole("button", { name: "View modified contacts" }));
    expect(navigateMock).toHaveBeenCalledWith("/group/imessage-import-2026-09-09");

    await user.click(screen.getByRole("button", { name: "← Back" }));
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it("offers Back only once the run is finished", () => {
    renderView();
    expect(screen.queryByRole("button", { name: "← Back" })).not.toBeInTheDocument();
  });

  it("offers only Back after a failed run", () => {
    renderView({
      phase: "done",
      running: false,
      summaryView: finished({ status: "failed" }),
      importSessionId: 42,
      completionText: "Import failed",
    });
    expect(screen.getByRole("heading", { name: "Import failed" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "← Back" })).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "View imported conversations" }),
    ).not.toBeInTheDocument();
    expect(getImportMock).not.toHaveBeenCalled();
  });

  it("shows errors in the table under the list and nowhere in the list", () => {
    renderView({
      phase: "done",
      running: false,
      steps: stepsAt("convert", { Staging: "done", Media: "done", Upload: "error" }),
      summaryView: finished({
        status: "completed_with_issues",
        issues: [{ kind: "warn", step: "upload", item: "chat.jsonl", reason: "Skipped one" }],
      }),
      importSessionId: null,
      completionText: "Import completed with issues",
    });
    expect(screen.getByRole("heading", { name: /^Errors/ })).toBeInTheDocument();
    expect(within(stageRow("Upload")).queryByText(/Errors/)).not.toBeInTheDocument();
  });

  it("has no errors section when the run reported none", () => {
    renderView({ phase: "done", running: false, summaryView: finished() });
    expect(screen.queryByRole("heading", { name: /^Errors/ })).not.toBeInTheDocument();
  });
});

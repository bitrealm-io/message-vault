/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ActiveImportSession } from "../../lib/importSession";
import ResumeImportPanel from "./ResumeImportPanel";
import type { ResumeDecision } from "./resumeDecision";

afterEach(() => {
  cleanup();
});

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

describe("ResumeImportPanel", () => {
  it("renders nothing when there is no session to decide about", () => {
    const decision: ResumeDecision = { kind: "none", session: null };
    const { container } = render(
      <ResumeImportPanel decision={decision} onResume={vi.fn()} onDiscard={vi.fn()} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("offers to resume the upload and calls back on the user's choice", async () => {
    const user = userEvent.setup();
    const onResume = vi.fn();
    const onDiscard = vi.fn();
    const decision: ResumeDecision = { kind: "resume_push", session: session() };
    render(<ResumeImportPanel decision={decision} onResume={onResume} onDiscard={onDiscard} />);

    expect(screen.getByText("Finish your last import")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Your messages are staged and ready to upload. Picking up where you left off skips the extract.",
      ),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Upload to vault" }));
    expect(onResume).toHaveBeenCalledTimes(1);
    expect(onDiscard).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Discard this import" }));
    expect(onDiscard).toHaveBeenCalledTimes(1);
  });

  it("offers to start over when the extract never finished", async () => {
    const user = userEvent.setup();
    const onResume = vi.fn();
    const onDiscard = vi.fn();
    const decision: ResumeDecision = {
      kind: "restart",
      session: session({ stage: "write" }),
    };
    render(<ResumeImportPanel decision={decision} onResume={onResume} onDiscard={onDiscard} />);

    expect(screen.getByText("Pick up your last import")).toBeInTheDocument();
    expect(
      screen.getByText(
        "The extract did not finish. Starting again reuses your settings and reads the backup from the beginning.",
      ),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Start over" }));
    expect(onResume).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: "Discard this import" }));
    expect(onDiscard).toHaveBeenCalledTimes(1);
  });

  it("offers to show the summary again for a session waiting at a gate", async () => {
    const user = userEvent.setup();
    const onResume = vi.fn();
    const onDiscard = vi.fn();
    const decision: ResumeDecision = {
      kind: "resume_gate",
      session: session({ stage: "awaiting_gate_1" }),
    };
    render(<ResumeImportPanel decision={decision} onResume={onResume} onDiscard={onDiscard} />);

    expect(screen.getByText("Pick up where you left off")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Your messages are staged. Opening the import again shows you the same summary, read fresh from the folder.",
      ),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Show me the summary" }));
    expect(onResume).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: "Discard this import" }));
    expect(onDiscard).toHaveBeenCalledTimes(1);
  });

  it("offers to carry on for a session that died mid media pass", async () => {
    const user = userEvent.setup();
    const onResume = vi.fn();
    const onDiscard = vi.fn();
    const decision: ResumeDecision = {
      kind: "resume_media",
      session: session({ stage: "transcode" }),
    };
    render(<ResumeImportPanel decision={decision} onResume={onResume} onDiscard={onDiscard} />);

    expect(screen.getByText("Finish preparing your media")).toBeInTheDocument();
    expect(
      screen.getByText(
        "The media step did not finish. Carrying on picks up the files it had not reached yet.",
      ),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Carry on" }));
    expect(onResume).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: "Discard this import" }));
    expect(onDiscard).toHaveBeenCalledTimes(1);
  });

  it("offers discard alone, naming the path, when the staged files are gone", async () => {
    const user = userEvent.setup();
    const onResume = vi.fn();
    const onDiscard = vi.fn();
    const decision: ResumeDecision = {
      kind: "folder_missing",
      session: session({ staging_dir: "/home/u/message-vault/staging-260830" }),
    };
    render(<ResumeImportPanel decision={decision} onResume={onResume} onDiscard={onDiscard} />);

    expect(screen.getByText("The staged files are gone")).toBeInTheDocument();
    expect(
      screen.getByText(
        "This import's folder is no longer at /home/u/message-vault/staging-260830. Discarding it lets you start a new one.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Upload to vault" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Start over" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Discard this import" }));
    expect(onDiscard).toHaveBeenCalledTimes(1);
    expect(onResume).not.toHaveBeenCalled();
  });

  it("does not name a path when the session never recorded one", async () => {
    const user = userEvent.setup();
    const onResume = vi.fn();
    const onDiscard = vi.fn();
    // Every Import Run created outside the desktop app — the CLI importer,
    // a tool of someone's own — stores a null staging_dir.
    const decision: ResumeDecision = {
      kind: "folder_missing",
      session: session({ staging_dir: null }),
    };
    render(<ResumeImportPanel decision={decision} onResume={onResume} onDiscard={onDiscard} />);

    expect(screen.getByText("There is nothing staged to pick up")).toBeInTheDocument();
    expect(
      screen.getByText(
        "This import did not record a staged folder, so there is nothing here to carry on from. Discarding it lets you start a new one.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText(/no longer at/)).not.toBeInTheDocument();
    expect(screen.queryByText(/null/)).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Discard this import" }));
    expect(onDiscard).toHaveBeenCalledTimes(1);
    expect(onResume).not.toHaveBeenCalled();
  });

  it("says the folder could not be checked rather than calling it gone", async () => {
    const user = userEvent.setup();
    const onResume = vi.fn();
    const onDiscard = vi.fn();
    const decision: ResumeDecision = {
      kind: "folder_unknown",
      session: session({ staging_dir: "/home/u/message-vault/staging-260830" }),
    };
    render(<ResumeImportPanel decision={decision} onResume={onResume} onDiscard={onDiscard} />);

    expect(screen.getByText("The staged files could not be checked")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Message Vault could not check /home/u/message-vault/staging-260830. Open Import again to check once more, or discard this import to start a new one.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText(/gone/)).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Discard this import" }));
    expect(onDiscard).toHaveBeenCalledTimes(1);
    expect(onResume).not.toHaveBeenCalled();
  });

  it("offers discard alone when the session belongs to another install", async () => {
    const user = userEvent.setup();
    const onResume = vi.fn();
    const onDiscard = vi.fn();
    const decision: ResumeDecision = {
      kind: "other_device",
      session: session({ device_id: "other-device" }),
    };
    render(<ResumeImportPanel decision={decision} onResume={onResume} onDiscard={onDiscard} />);

    expect(screen.getByText("This import belongs to another computer")).toBeInTheDocument();
    expect(
      screen.getByText(
        "It was started on a different install and its files are staged there. Discarding it lets you start a new import here.",
      ),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Discard this import" }));
    expect(onDiscard).toHaveBeenCalledTimes(1);
    expect(onResume).not.toHaveBeenCalled();
  });

  it("offers discard alone when the stored settings can't be read", async () => {
    const user = userEvent.setup();
    const onResume = vi.fn();
    const onDiscard = vi.fn();
    const decision: ResumeDecision = {
      kind: "settings_unreadable",
      session: session(),
    };
    render(<ResumeImportPanel decision={decision} onResume={onResume} onDiscard={onDiscard} />);

    expect(screen.getByText("This import's settings could not be read")).toBeInTheDocument();
    expect(
      screen.getByText(
        "The import is still open here, but the settings it was started with are not readable. Discarding it lets you start a new one.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Upload to vault" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Start over" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Discard this import" }));
    expect(onDiscard).toHaveBeenCalledTimes(1);
    expect(onResume).not.toHaveBeenCalled();
  });

  it("says nothing extra when there is no error to report", () => {
    const decision: ResumeDecision = {
      kind: "resume_gate",
      session: session({ stage: "awaiting_gate_1" }),
    };
    render(<ResumeImportPanel decision={decision} onResume={vi.fn()} onDiscard={vi.fn()} />);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("surfaces a failed resume attempt without blocking the retry", async () => {
    const user = userEvent.setup();
    const onResume = vi.fn();
    const decision: ResumeDecision = {
      kind: "resume_gate",
      session: session({ stage: "awaiting_gate_1" }),
    };
    render(
      <ResumeImportPanel
        decision={decision}
        error="disk unavailable"
        onResume={onResume}
        onDiscard={vi.fn()}
      />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent("disk unavailable");
    // The panel's own copy for the decision still renders underneath.
    expect(screen.getByText("Pick up where you left off")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Show me the summary" }));
    expect(onResume).toHaveBeenCalledTimes(1);
  });
  it("offers to pick up a copy that did not finish", async () => {
    const user = userEvent.setup();
    const onResume = vi.fn();
    const onDiscard = vi.fn();
    const decision: ResumeDecision = {
      kind: "resume_write",
      session: session({ stage: "write" }),
    };
    render(<ResumeImportPanel decision={decision} onResume={onResume} onDiscard={onDiscard} />);

    expect(screen.getByText("Finish copying your backup")).toBeInTheDocument();
    expect(
      screen.getByText(
        "The copy did not finish. Picking up where you left off reads the backup again and skips the conversations already copied.",
      ),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Pick up" }));
    expect(onResume).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: "Discard this import" }));
    expect(onDiscard).toHaveBeenCalledTimes(1);
  });

  it("names the backup that changed, and offers to start over", async () => {
    const user = userEvent.setup();
    const onResume = vi.fn();
    const onDiscard = vi.fn();
    const decision: ResumeDecision = {
      kind: "source_changed",
      session: session({
        stage: "write",
        source_fingerprint: {
          path: "/backups/iphone.tar",
          size_bytes: 10,
          modified_unix_ms: 1,
          message_count: null,
        },
      }),
    };
    render(<ResumeImportPanel decision={decision} onResume={onResume} onDiscard={onDiscard} />);

    expect(screen.getByText("The backup has changed")).toBeInTheDocument();
    expect(
      screen.getByText(
        "This import was reading /backups/iphone.tar, and that backup is different now. Starting over reads it fresh with the same settings.",
      ),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Start over" }));
    expect(onResume).toHaveBeenCalledTimes(1);
  });

  it("says the backup changed without a path when the session recorded none", () => {
    const decision: ResumeDecision = {
      kind: "source_changed",
      session: session({ stage: "write" }),
    };
    render(<ResumeImportPanel decision={decision} onResume={vi.fn()} onDiscard={vi.fn()} />);

    expect(
      screen.getByText(
        "The backup this import was reading is different now. Starting over reads it fresh with the same settings.",
      ),
    ).toBeInTheDocument();
  });
});

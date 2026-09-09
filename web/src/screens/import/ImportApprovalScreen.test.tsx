/** @vitest-environment jsdom */

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { StagingSummary } from "../../lib/tauri";
import type { AttachmentMediaMode } from "../../lib/types";
import type { GateDelta } from "./gateDelta";
import ImportApprovalScreen from "./ImportApprovalScreen";
import { type ImportStep, stepsFor } from "./importProgressState";
import { type ApprovalKind, approvalSteps } from "./importRunCopy";

afterEach(() => {
  cleanup();
});

function summary(overrides: Partial<StagingSummary> = {}): StagingSummary {
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

function delta(overrides: Partial<GateDelta> = {}): GateDelta {
  return {
    lostCount: 0,
    stillFlagged: [],
    cameOutFine: 0,
    hasChanges: false,
    ...overrides,
  };
}

/** Rows as the hook leaves them at an approval: Staging done, the rest pending. */
function stagedSteps(mode: AttachmentMediaMode, mediaDone = false): ImportStep[] {
  return stepsFor(mode).map((step) => {
    if (step.label === "Staging") return { ...step, status: "done" };
    if (step.label === "Media" && mediaDone) return { ...step, status: "done" };
    return step;
  });
}

function props(
  overrides: {
    kind?: ApprovalKind;
    summary?: Partial<StagingSummary>;
    delta?: Partial<GateDelta> | null;
    unknownContacts?: number | null;
    mode?: AttachmentMediaMode;
    onApprove?: () => void;
    onCancel?: () => void;
    onBack?: () => void;
    busy?: boolean;
    mediaToolsMissing?: boolean;
    mediaPartiallyRan?: boolean;
  } = {},
) {
  const kind = overrides.kind ?? "staging";
  const mode = overrides.mode ?? "convert";
  return {
    kind,
    steps: stagedSteps(mode, kind === "media"),
    summary: summary(overrides.summary),
    delta: overrides.delta === null ? null : kind === "media" ? delta(overrides.delta) : null,
    unknownContacts: overrides.unknownContacts === undefined ? 0 : overrides.unknownContacts,
    mode,
    onApprove: overrides.onApprove ?? vi.fn(),
    onCancel: overrides.onCancel ?? vi.fn(),
    onBack: overrides.onBack ?? vi.fn(),
    busy: overrides.busy ?? false,
    mediaToolsMissing: overrides.mediaToolsMissing ?? false,
    mediaPartiallyRan: overrides.mediaPartiallyRan ?? false,
  };
}

describe("approvalSteps", () => {
  it("marks the stage the Staging Approval guards", () => {
    expect(approvalSteps(stagedSteps("convert"), "staging")).toMatchObject([
      { label: "Staging", status: "done" },
      { label: "Media", detail: "Needs your approval" },
      { label: "Upload", status: "pending" },
    ]);
    expect(approvalSteps(stagedSteps("copy"), "staging")[1]).toMatchObject({
      label: "Upload",
      detail: "Needs your approval",
    });
  });

  it("marks Upload at the Media Approval", () => {
    expect(approvalSteps(stagedSteps("convert", true), "media")[2]).toMatchObject({
      label: "Upload",
      detail: "Needs your approval",
    });
  });
});

describe("ImportApprovalScreen at the Staging Approval", () => {
  it("asks for approval of what was staged", () => {
    render(<ImportApprovalScreen {...props()} />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
      "Approve the staged import",
    );
    expect(screen.getByText("Needs your approval")).toBeInTheDocument();
  });

  it("shows the measured counts", () => {
    render(
      <ImportApprovalScreen
        {...props({
          summary: {
            conversations: 12,
            messages: 4310,
            attachments: 88,
            attachmentBytes: 1024 * 1024 * 512,
          },
        })}
      />,
    );
    expect(screen.getByText("12")).toBeInTheDocument();
    expect(screen.getByText("4,310")).toBeInTheDocument();
  });

  it("says how many contacts are new to the vault", () => {
    render(<ImportApprovalScreen {...props({ unknownContacts: 7 })} />);
    expect(screen.getByText(/7 new to your vault/)).toBeInTheDocument();
  });

  it("omits the new-to-vault clause when the contact lookup failed", () => {
    render(<ImportApprovalScreen {...props({ unknownContacts: null })} />);
    expect(screen.queryByText(/new to your vault/)).not.toBeInTheDocument();
  });

  it("says the media step has not run yet in the genuine not-yet-run case", () => {
    render(<ImportApprovalScreen {...props({ mode: "convert", mediaPartiallyRan: false })} />);
    expect(
      screen.getByText(
        "The media step has not run yet, so these are estimates based on the files as staged.",
      ),
    ).toBeInTheDocument();
  });

  it("does not claim the media step hasn't run when a resume found it partway through", () => {
    render(<ImportApprovalScreen {...props({ mode: "convert", mediaPartiallyRan: true })} />);
    expect(screen.queryByText(/has not run yet/)).not.toBeInTheDocument();
    expect(
      screen.getByText(
        "The media step needs its tools to finish. Approving here picks up where it left off, once they're available.",
      ),
    ).toBeInTheDocument();
  });

  it("offers to start the media step under convert", () => {
    render(<ImportApprovalScreen {...props({ mode: "convert" })} />);
    expect(screen.getByRole("button", { name: "Convert media" })).toBeInTheDocument();
  });

  it("offers to upload directly under copy, because there is no media step", () => {
    render(<ImportApprovalScreen {...props({ mode: "copy" })} />);
    expect(screen.getByRole("button", { name: "Upload to vault" })).toBeInTheDocument();
    expect(screen.queryByText(/estimate/i)).not.toBeInTheDocument();
  });

  it("disables approval and says ffmpeg is needed when the tools are missing under convert", () => {
    render(<ImportApprovalScreen {...props({ mode: "convert", mediaToolsMissing: true })} />);
    expect(screen.getByRole("button", { name: "Convert media" })).toBeDisabled();
    expect(screen.getByText(/ffmpeg/i)).toBeInTheDocument();
  });

  it("does not gate copy mode on missing ffmpeg tools, which it never needs", () => {
    render(<ImportApprovalScreen {...props({ mode: "copy", mediaToolsMissing: true })} />);
    expect(screen.getByRole("button", { name: "Upload to vault" })).not.toBeDisabled();
    expect(screen.queryByText(/ffmpeg/i)).not.toBeInTheDocument();
  });

  it("shows the over-limit breakdown under copy mode too, naming the limit instead of a media step", () => {
    render(
      <ImportApprovalScreen
        {...props({
          mode: "copy",
          summary: {
            verdictCounts: {
              fitsAsIs: 3,
              likelyFits: 0,
              mayGrow: 0,
              probablyTooBig: 2,
              cannotProcess: 0,
            },
          },
        })}
      />,
    );
    expect(screen.getByText(/2 files — Over the size limit/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 2 })).toHaveTextContent(/upload limit/i);
  });

  it("renders the identity panel it is given", () => {
    render(
      <ImportApprovalScreen {...props()} identityPanel={<div data-testid="identity-panel" />} />,
    );
    expect(screen.getByTestId("identity-panel")).toBeInTheDocument();
  });

  it("keeps the breakdown's singular file count correct", () => {
    render(
      <ImportApprovalScreen
        {...props({
          mode: "convert",
          summary: {
            verdictCounts: {
              fitsAsIs: 0,
              likelyFits: 0,
              mayGrow: 0,
              probablyTooBig: 1,
              cannotProcess: 0,
            },
          },
        })}
      />,
    );
    expect(screen.getByText(/^1 file — /)).toBeInTheDocument();
  });
});

describe("ImportApprovalScreen at the Media Approval", () => {
  it("asks for approval of the converted media and leads with what changed", () => {
    render(
      <ImportApprovalScreen
        {...props({ kind: "media", delta: { lostCount: 2, hasChanges: true } })}
      />,
    );
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
      "Approve the converted media",
    );
    const headings = screen.getAllByRole("heading");
    expect(headings[1]).toHaveTextContent(/what changed/i);
    expect(screen.getByText("2 files will not be uploaded.")).toBeInTheDocument();
  });

  it("says compressed when that was the job", () => {
    render(<ImportApprovalScreen {...props({ kind: "media", mode: "compress" })} />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
      "Approve the compressed media",
    );
  });

  it("says so plainly when the forecast held", () => {
    render(<ImportApprovalScreen {...props({ kind: "media", delta: { hasChanges: false } })} />);
    expect(screen.getByText(/came out as expected/i)).toBeInTheDocument();
  });

  it("still lists a file that will not upload when nothing else changed", () => {
    render(
      <ImportApprovalScreen
        {...props({
          kind: "media",
          delta: {
            hasChanges: false,
            stillFlagged: [{ name: "a.mov", verdict: "cannot_process", regressed: false }],
          },
        })}
      />,
    );
    expect(screen.getByText(/came out as expected/i)).toBeInTheDocument();
    expect(screen.getByText(/^1 file — /)).toBeInTheDocument();
  });

  it("only ever offers to upload", () => {
    render(<ImportApprovalScreen {...props({ kind: "media", mode: "convert" })} />);
    expect(screen.getByRole("button", { name: "Upload to vault" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Convert media" })).not.toBeInTheDocument();
  });
});

describe("ImportApprovalScreen acting on a click", () => {
  it("approves when the button is pressed and nothing is in flight", () => {
    const onApprove = vi.fn();
    const onCancel = vi.fn();
    render(<ImportApprovalScreen {...props({ onApprove, onCancel })} />);
    fireEvent.click(screen.getByRole("button", { name: "Convert media" }));
    expect(onApprove).toHaveBeenCalledTimes(1);
    expect(onCancel).not.toHaveBeenCalled();
  });

  it("cancels the import when asked, and does not approve", () => {
    const onApprove = vi.fn();
    const onCancel = vi.fn();
    render(<ImportApprovalScreen {...props({ onApprove, onCancel })} />);
    fireEvent.click(screen.getByRole("button", { name: "Cancel this import" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onApprove).not.toHaveBeenCalled();
  });

  it("goes back to the run without deciding", () => {
    const onApprove = vi.fn();
    const onCancel = vi.fn();
    const onBack = vi.fn();
    render(<ImportApprovalScreen {...props({ onApprove, onCancel, onBack })} />);
    fireEvent.click(screen.getByRole("button", { name: "← Back to the run" }));
    expect(onBack).toHaveBeenCalledTimes(1);
    expect(onApprove).not.toHaveBeenCalled();
    expect(onCancel).not.toHaveBeenCalled();
  });

  it("does nothing on either button while busy", () => {
    const onApprove = vi.fn();
    const onCancel = vi.fn();
    render(<ImportApprovalScreen {...props({ onApprove, onCancel, busy: true })} />);
    fireEvent.click(screen.getByRole("button", { name: "Convert media" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel this import" }));
    expect(onApprove).not.toHaveBeenCalled();
    expect(onCancel).not.toHaveBeenCalled();
  });
});

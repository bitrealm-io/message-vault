/** @vitest-environment jsdom */

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { StagingSummary } from "../../lib/tauri";
import type { AttachmentMediaMode } from "../../lib/types";
import GateTwoScreen from "./GateTwoScreen";
import type { GateDelta } from "./gateDelta";

afterEach(() => {
  cleanup();
});

function delta(overrides: Partial<GateDelta> = {}): GateDelta {
  return {
    lostCount: 0,
    stillFlagged: [],
    cameOutFine: 0,
    hasChanges: false,
    ...overrides,
  };
}

function actual(overrides: Partial<StagingSummary> = {}): StagingSummary {
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

function props(
  overrides: {
    delta?: Partial<GateDelta>;
    actual?: Partial<StagingSummary>;
    mode?: AttachmentMediaMode;
    onApprove?: () => void;
    onDecline?: () => void;
    busy?: boolean;
  } = {},
) {
  return {
    delta: delta(overrides.delta),
    actual: actual(overrides.actual),
    mode: overrides.mode ?? "convert",
    onApprove: overrides.onApprove ?? vi.fn(),
    onDecline: overrides.onDecline ?? vi.fn(),
    busy: overrides.busy ?? false,
  };
}

describe("GateTwoScreen", () => {
  it("names the stage in its heading", () => {
    render(<GateTwoScreen {...props()} />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Ready to upload");
  });

  it("leads with the delta, not a fresh summary", () => {
    // Decision 14: where the last check's estimate was wrong. The final
    // state follows underneath.
    render(<GateTwoScreen {...props({ delta: { lostCount: 2, hasChanges: true } })} />);
    const headings = screen.getAllByRole("heading");
    expect(headings[1]).toHaveTextContent(/what changed/i);
  });

  it("says so plainly when the forecast held", () => {
    render(<GateTwoScreen {...props({ delta: { hasChanges: false } })} />);
    expect(screen.getByText(/came out as expected/i)).toBeInTheDocument();
  });

  it("carries the standing copy about what an import does", () => {
    render(<GateTwoScreen {...props()} />);
    expect(
      screen.getByText(
        "Messages are always uploaded. A skipped attachment leaves a placeholder in the conversation, and the message text is kept. Imported conversations can later be removed from your vault in the messages area.",
      ),
    ).toBeInTheDocument();
  });

  it("offers to upload and to cancel", () => {
    render(<GateTwoScreen {...props()} />);
    expect(screen.getByRole("button", { name: "Upload to vault" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Cancel this import" })).toBeInTheDocument();
  });

  it("shows the recomputed counts, not the approved ones", () => {
    render(
      <GateTwoScreen
        {...props({
          actual: { conversations: 9, messages: 501, attachments: 42, attachmentBytes: 2048 },
        })}
      />,
    );
    expect(screen.getByText("9")).toBeInTheDocument();
    expect(screen.getByText("501")).toBeInTheDocument();
    expect(screen.getByText("42")).toBeInTheDocument();
  });

  it("says what happened to the lost files, without naming a cause", () => {
    // Decision 45: too_large and convert_failed are indistinguishable from
    // the recomputed summary, so the copy states the effect, not a cause.
    render(<GateTwoScreen {...props({ delta: { lostCount: 2, hasChanges: true } })} />);
    expect(screen.getByText(/will not be uploaded/i)).toBeInTheDocument();
  });

  it("does not say 'will not be uploaded' when nothing was lost", () => {
    render(
      <GateTwoScreen {...props({ delta: { lostCount: 0, cameOutFine: 3, hasChanges: true } })} />,
    );
    expect(screen.queryByText(/will not be uploaded/i)).not.toBeInTheDocument();
  });

  it("shows a still-pending row alongside 'came out as expected', not hidden behind it", () => {
    // A cannot_process file is never touched by the media step in any mode,
    // so it is common for `stillFlagged` to hold only non-regressed rows —
    // no delta, but the file still will not upload. That must not vanish
    // behind the "no surprises" line.
    render(
      <GateTwoScreen
        {...props({
          delta: {
            hasChanges: false,
            stillFlagged: [{ name: "archive.zip", verdict: "cannot_process", regressed: false }],
          },
        })}
      />,
    );
    expect(screen.getByText(/came out as expected/i)).toBeInTheDocument();
    expect(screen.getByText(/not audio or video/i)).toBeInTheDocument();
  });

  it("gives a regressed row decision 45's framing, not 'could not be processed'", () => {
    render(
      <GateTwoScreen
        {...props({
          delta: {
            hasChanges: true,
            stillFlagged: [{ name: "clip.mov", verdict: "probably_too_big", regressed: true }],
          },
        })}
      />,
    );
    expect(screen.getByText(/fine at the last check/i)).toBeInTheDocument();
    expect(screen.queryByText(/could not be processed/i)).not.toBeInTheDocument();
  });

  it("never says Gate 1", () => {
    render(<GateTwoScreen {...props()} />);
    expect(document.body.textContent).not.toContain("Gate 1");
  });

  it("never says transcode", () => {
    render(<GateTwoScreen {...props()} />);
    expect(document.body.textContent?.toLowerCase()).not.toContain("transcode");
  });

  it("does not act twice on a double click", () => {
    const onApprove = vi.fn();
    render(<GateTwoScreen {...props({ onApprove, busy: true })} />);
    fireEvent.click(screen.getByRole("button", { name: "Upload to vault" }));
    expect(onApprove).not.toHaveBeenCalled();
  });

  it("does not act twice on a double click of the decline button either", () => {
    const onDecline = vi.fn();
    render(<GateTwoScreen {...props({ onDecline, busy: true })} />);
    fireEvent.click(screen.getByRole("button", { name: "Cancel this import" }));
    expect(onDecline).not.toHaveBeenCalled();
  });

  it("keeps the singular file count correct in the lost-files line", () => {
    render(<GateTwoScreen {...props({ delta: { lostCount: 1, hasChanges: true } })} />);
    expect(screen.getByText("1 file will not be uploaded.")).toBeInTheDocument();
  });
});

/**
 * The ordinary click, which nothing tested.
 *
 * Both buttons were only ever pressed with `busy: true`, asserting that
 * nothing happened. That is half a test: a screen whose buttons were wired to
 * nothing satisfied it exactly as well as one that works, and the gate is the
 * step where a person says "yes, upload my messages".
 */
describe("GateTwoScreen acting on an ordinary click", () => {
  it("approves the upload when the button is pressed", () => {
    const onApprove = vi.fn();
    const onDecline = vi.fn();
    render(<GateTwoScreen {...props({ onApprove, onDecline })} />);

    fireEvent.click(screen.getByRole("button", { name: "Upload to vault" }));

    expect(onApprove).toHaveBeenCalledTimes(1);
    expect(onDecline).not.toHaveBeenCalled();
  });

  it("declines when Cancel is pressed, and does not approve", () => {
    const onApprove = vi.fn();
    const onDecline = vi.fn();
    render(<GateTwoScreen {...props({ onApprove, onDecline })} />);

    fireEvent.click(screen.getByRole("button", { name: "Cancel this import" }));

    expect(onDecline).toHaveBeenCalledTimes(1);
    expect(onApprove).not.toHaveBeenCalled();
  });
});

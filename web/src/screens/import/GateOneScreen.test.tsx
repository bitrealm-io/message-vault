/** @vitest-environment jsdom */

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { StagingSummary } from "../../lib/tauri";
import type { AttachmentMediaMode } from "../../lib/types";
import GateOneScreen from "./GateOneScreen";

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

function props(
  overrides: {
    summary?: Partial<StagingSummary>;
    unknownContacts?: number | null;
    mode?: AttachmentMediaMode;
    onApprove?: () => void;
    onDecline?: () => void;
    busy?: boolean;
    mediaToolsMissing?: boolean;
    mediaPartiallyRan?: boolean;
  } = {},
) {
  return {
    summary: summary(overrides.summary),
    unknownContacts: overrides.unknownContacts === undefined ? 0 : overrides.unknownContacts,
    mode: overrides.mode ?? "convert",
    onApprove: overrides.onApprove ?? vi.fn(),
    onDecline: overrides.onDecline ?? vi.fn(),
    busy: overrides.busy ?? false,
    mediaToolsMissing: overrides.mediaToolsMissing ?? false,
    mediaPartiallyRan: overrides.mediaPartiallyRan ?? false,
  };
}

describe("GateOneScreen", () => {
  it("names the stage in its heading", () => {
    render(<GateOneScreen {...props()} />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Review what was copied");
  });

  it("shows the measured counts", () => {
    render(
      <GateOneScreen
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
    render(<GateOneScreen {...props({ unknownContacts: 7 })} />);
    expect(screen.getByText(/7 new to your vault/)).toBeInTheDocument();
  });

  it("omits the new-to-vault clause when the contact lookup failed", () => {
    render(<GateOneScreen {...props({ unknownContacts: null })} />);
    expect(screen.queryByText(/new to your vault/)).not.toBeInTheDocument();
  });

  it("says the size numbers are estimates", () => {
    // The screen says throughout that these are estimates (decision 13).
    render(<GateOneScreen {...props({ mode: "convert" })} />);
    expect(screen.getByText(/estimate/i)).toBeInTheDocument();
  });

  it("says the media step has not run yet in the genuine not-yet-run case", () => {
    render(<GateOneScreen {...props({ mode: "convert", mediaPartiallyRan: false })} />);
    expect(
      screen.getByText(
        "The media step has not run yet, so these are estimates based on the files as staged.",
      ),
    ).toBeInTheDocument();
  });

  it("does not claim the media step hasn't run when a resume found it partway through", () => {
    // A resume that landed here because ffmpeg went missing mid pass may
    // have a folder that already holds some converted files -- the
    // not-yet-run sentence would be false there.
    render(<GateOneScreen {...props({ mode: "convert", mediaPartiallyRan: true })} />);
    expect(screen.queryByText(/has not run yet/)).not.toBeInTheDocument();
    expect(
      screen.getByText(
        "The media step needs its tools to finish. Approving here picks up where it left off, once they're available.",
      ),
    ).toBeInTheDocument();
  });

  it("offers to start the media step under convert", () => {
    render(<GateOneScreen {...props({ mode: "convert" })} />);
    expect(screen.getByRole("button", { name: "Convert media" })).toBeInTheDocument();
  });

  it("offers to upload directly under copy, because there is no media step", () => {
    render(<GateOneScreen {...props({ mode: "copy" })} />);
    expect(screen.getByRole("button", { name: "Upload to vault" })).toBeInTheDocument();
    expect(screen.queryByText(/estimate/i)).not.toBeInTheDocument();
  });

  it("does not act twice on a double click", () => {
    const onApprove = vi.fn();
    render(<GateOneScreen {...props({ onApprove, busy: true })} />);
    fireEvent.click(screen.getByRole("button", { name: /Convert media|Upload to vault/ }));
    expect(onApprove).not.toHaveBeenCalled();
  });

  it("does not act twice on a double click of the decline button either", () => {
    // The brief only pins this for approve; busy gates both buttons, so a
    // double-click on cancel must be a no-op too (decline callback never runs).
    const onDecline = vi.fn();
    render(<GateOneScreen {...props({ onDecline, busy: true })} />);
    fireEvent.click(screen.getByRole("button", { name: "Cancel this import" }));
    expect(onDecline).not.toHaveBeenCalled();
  });

  it("offers to cancel the import", () => {
    render(<GateOneScreen {...props()} />);
    expect(screen.getByRole("button", { name: "Cancel this import" })).toBeInTheDocument();
  });

  it("disables approval and says ffmpeg is needed when the tools are missing under convert", () => {
    render(<GateOneScreen {...props({ mode: "convert", mediaToolsMissing: true })} />);
    expect(screen.getByRole("button", { name: "Convert media" })).toBeDisabled();
    expect(screen.getByText(/ffmpeg/i)).toBeInTheDocument();
  });

  it("does not gate copy mode on missing ffmpeg tools, which it never needs", () => {
    render(<GateOneScreen {...props({ mode: "copy", mediaToolsMissing: true })} />);
    expect(screen.getByRole("button", { name: "Upload to vault" })).not.toBeDisabled();
    expect(screen.queryByText(/ffmpeg/i)).not.toBeInTheDocument();
  });

  it("shows the over-limit breakdown under copy mode too, naming the limit instead of a media step", () => {
    // Copy/skip has no media step, but decision 11's breakdown was only ever
    // rendered when a verb existed — the exact verdicts on a copy-mode
    // import (which files will not fit) were silently dropped.
    render(
      <GateOneScreen
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
    expect(screen.queryByText(/estimate/i)).not.toBeInTheDocument();
  });

  it("renders the identity panel it is given", () => {
    render(<GateOneScreen {...props()} identityPanel={<div data-testid="identity-panel" />} />);
    expect(screen.getByTestId("identity-panel")).toBeInTheDocument();
  });

  it("keeps the breakdown's singular file count correct", () => {
    render(
      <GateOneScreen
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

/**
 * The ordinary click, which nothing tested.
 *
 * Both buttons were only ever pressed with `busy: true`, asserting that
 * nothing happened. That is half a test: a screen whose buttons were wired to
 * nothing satisfied it exactly as well as one that works, and the gate is the
 * step where a person says "yes, upload my messages".
 */
describe("GateOneScreen acting on an ordinary click", () => {
  it("approves when the button is pressed and nothing is in flight", () => {
    const onApprove = vi.fn();
    const onDecline = vi.fn();
    render(<GateOneScreen {...props({ onApprove, onDecline })} />);

    fireEvent.click(screen.getByRole("button", { name: /Convert media|Upload to vault/ }));

    expect(onApprove).toHaveBeenCalledTimes(1);
    expect(onDecline).not.toHaveBeenCalled();
  });

  it("declines when Cancel is pressed, and does not approve", () => {
    const onApprove = vi.fn();
    const onDecline = vi.fn();
    render(<GateOneScreen {...props({ onApprove, onDecline })} />);

    fireEvent.click(screen.getByRole("button", { name: "Cancel this import" }));

    expect(onDecline).toHaveBeenCalledTimes(1);
    expect(onApprove).not.toHaveBeenCalled();
  });
});

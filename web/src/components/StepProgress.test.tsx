/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import StepProgress, { type Step } from "./StepProgress";

const doneSteps: Step[] = [{ label: "Upload to vault", status: "done" }];

describe("StepProgress completion badge", () => {
  afterEach(() => {
    cleanup();
  });

  it("gives completed-with-issues its own badge, not the canceled/muted one", () => {
    render(<StepProgress steps={doneSteps} completionText="Import completed with issues" />);
    const text = screen.getByText("Import completed with issues");
    const badge = text.previousElementSibling;
    expect(badge).not.toBeNull();
    expect(badge?.className).toContain("bg-warn-soft-bg");
    expect(badge?.className).not.toContain("bg-border");
  });

  it("still gives a canceled/other completion the muted badge", () => {
    render(<StepProgress steps={doneSteps} completionText="Import canceled" />);
    const text = screen.getByText("Import canceled");
    const badge = text.previousElementSibling;
    expect(badge?.className).toContain("bg-border");
  });

  it("keeps the ok badge for a clean completion", () => {
    render(<StepProgress steps={doneSteps} completionText="Import complete" />);
    const text = screen.getByText("Import complete");
    const badge = text.previousElementSibling;
    expect(badge?.className).toContain("bg-ok");
  });
});

describe("StepProgress wide list", () => {
  afterEach(() => {
    cleanup();
  });

  it("puts each step's content under its label and its duration or note beside it", () => {
    render(
      <StepProgress
        wide
        steps={[
          {
            label: "Staging",
            status: "done",
            durationMs: 46_000,
            content: <p>681 conversations</p>,
          },
          { label: "Staging Approval", status: "done", note: "Approved" },
          { label: "Upload", status: "pending" },
        ]}
      />,
    );
    const [staging, approval, upload] = screen.getAllByRole("listitem");
    expect(staging).toHaveTextContent("46s");
    expect(staging).toHaveTextContent("681 conversations");
    expect(approval).toHaveTextContent("Approved");
    expect(upload).toHaveTextContent("3");
  });

  it("marks a row that is waiting on the person as the current step", () => {
    render(
      <StepProgress
        wide
        steps={[
          { label: "Staging", status: "done" },
          { label: "Staging Approval", status: "waiting" },
        ]}
      />,
    );
    const [staging, approval] = screen.getAllByRole("listitem");
    expect(staging).not.toHaveAttribute("aria-current");
    expect(approval).toHaveAttribute("aria-current", "step");
  });
});

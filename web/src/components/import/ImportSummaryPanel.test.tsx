/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import ImportSummaryPanel, {
  completionTextFor,
  type ImportSummaryView,
} from "./ImportSummaryPanel";

const baseSummary: ImportSummaryView = {
  status: "completed",
  messagesParsed: 10,
  messagesAttempted: 10,
  messagesInserted: 9,
  messagesDeduped: 1,
  messagesFailed: 0,
  durationMs: 1000,
  issues: [],
};

describe("ImportSummaryPanel", () => {
  afterEach(() => {
    cleanup();
  });

  it("shows four history steps including Attachments and Preparing messages", () => {
    render(
      <ImportSummaryPanel
        summary={{
          ...baseSummary,
          parseMs: 10,
          attachmentsMs: 20,
          prepareMs: 5,
          uploadMs: 30,
        }}
      />,
    );
    expect(screen.getByText("Parse backup")).toBeInTheDocument();
    expect(screen.getByText("Attachments")).toBeInTheDocument();
    expect(screen.getByText("Preparing messages")).toBeInTheDocument();
    expect(screen.getByText("Upload to Message Crate")).toBeInTheDocument();
    expect(screen.queryByText("Convert attachments")).not.toBeInTheDocument();
  });

  it("hides Import Errors when there are no issues", () => {
    render(<ImportSummaryPanel summary={baseSummary} embedStepTimings={false} />);
    expect(screen.queryByRole("heading", { name: "Import Errors" })).not.toBeInTheDocument();
    expect(screen.queryByText("Open import log")).not.toBeInTheDocument();
  });

  it("shows Import Errors heading and table when issues exist", () => {
    render(
      <ImportSummaryPanel
        summary={{
          ...baseSummary,
          issues: [
            {
              kind: "error",
              stage: "upload",
              item: "thread.jsonl",
              reason: "HTTP 500 from server",
            },
          ],
        }}
        embedStepTimings={false}
      />,
    );
    expect(screen.getByRole("heading", { name: "Import Errors" })).toBeInTheDocument();
    expect(
      screen.getByText(
        "Identical errors are grouped. Error messages show two lines. Click a row to expand the full message and the file list.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Import errors")).toBeInTheDocument();
    expect(screen.queryByText("Open import log")).not.toBeInTheDocument();
  });
});

describe("completionTextFor", () => {
  it("names the with-issues outcome", () => {
    expect(completionTextFor("completed_with_issues")).toBe("Import completed with issues");
  });

  it("names a paused Upload as paused, not cancelled", () => {
    expect(completionTextFor("paused")).toBe("Import paused");
  });
});

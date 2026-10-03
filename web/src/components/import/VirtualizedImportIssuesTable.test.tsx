/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ImportIssue } from "./ImportSummaryPanel";
import { estimateExpandedHeight, tableViewportHeight } from "./importIssuesTableLayout";
import VirtualizedImportIssuesTable from "./VirtualizedImportIssuesTable";

/**
 * jsdom lays out nothing, so React Aria's Virtualizer would draw no rows. Every
 * element reports a 600px box, enough for the rows these tests draw.
 */
let restoreLayout: () => void = () => {};
beforeEach(() => {
  const heights = vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  const widths = vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(600);
  restoreLayout = () => {
    heights.mockRestore();
    widths.mockRestore();
  };
});

afterEach(() => {
  restoreLayout();
  cleanup();
});

function issue(partial: Partial<ImportIssue> & Pick<ImportIssue, "item" | "reason">): ImportIssue {
  return {
    kind: "error",
    stage: "upload",
    ...partial,
  };
}

describe("VirtualizedImportIssuesTable", () => {
  it("names the Stage an error happened in", () => {
    render(
      <VirtualizedImportIssuesTable
        issues={[
          issue({ item: "a.jsonl", reason: "could not read", stage: "staging" }),
          issue({ item: "b.jpg", reason: "ffmpeg failed", stage: "media" }),
          issue({ item: "c.jsonl", reason: "HTTP 500 from server", stage: "upload" }),
        ]}
      />,
    );
    expect(screen.getByRole("columnheader", { name: "Stage" })).toBeInTheDocument();
    expect(screen.getByText("Staging")).toBeInTheDocument();
    expect(screen.getByText("Media")).toBeInTheDocument();
    expect(screen.getByText("Upload")).toBeInTheDocument();
  });

  it("shows the filename for a unique issue", () => {
    render(
      <VirtualizedImportIssuesTable
        issues={[issue({ item: "chat.jsonl", reason: "HTTP 500 from server" })]}
      />,
    );
    expect(screen.getByRole("grid", { name: "Import errors" })).toHaveAttribute(
      "aria-rowcount",
      "2",
    );
    expect(screen.getByText("chat.jsonl")).toBeInTheDocument();
    expect(screen.queryByText("1 files")).not.toBeInTheDocument();
  });

  it("shows N files for a group and lists names only after expand", async () => {
    const user = userEvent.setup();
    render(
      <VirtualizedImportIssuesTable
        issues={[
          issue({ item: "a.jsonl", reason: "source mismatch" }),
          issue({ item: "b.jsonl", reason: "source mismatch" }),
          issue({ item: "c.jsonl", reason: "source mismatch" }),
        ]}
      />,
    );
    expect(screen.getByRole("grid", { name: "Import errors" })).toHaveAttribute(
      "aria-rowcount",
      "2",
    );
    expect(screen.getByText("3 files")).toBeInTheDocument();
    expect(screen.queryByText("a.jsonl")).not.toBeInTheDocument();

    await user.click(screen.getByRole("row", { name: "3 files" }));

    expect(screen.getByRole("button", { name: "Collapse error for 3 files" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(screen.getByText("a.jsonl")).toBeInTheDocument();
    expect(screen.getByText("b.jsonl")).toBeInTheDocument();
    expect(screen.getByText("c.jsonl")).toBeInTheDocument();
    expect(screen.getByText("source mismatch")).toBeInTheDocument();
  });

  it("expands a unique row to the reason only", async () => {
    const user = userEvent.setup();
    render(
      <VirtualizedImportIssuesTable
        issues={[issue({ item: "chat.jsonl", reason: "HTTP 500 from server" })]}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Expand error for chat.jsonl" }));

    expect(screen.getByText("HTTP 500 from server")).toBeInTheDocument();
    expect(screen.getAllByText("chat.jsonl")).toHaveLength(1);
    expect(screen.queryByRole("list")).not.toBeInTheDocument();
  });

  it("keeps the filename list open when a name is clicked", async () => {
    const user = userEvent.setup();
    render(
      <VirtualizedImportIssuesTable
        issues={[
          issue({ item: "a.jsonl", reason: "source mismatch" }),
          issue({ item: "b.jsonl", reason: "source mismatch" }),
        ]}
      />,
    );

    await user.click(screen.getByRole("row", { name: "2 files" }));
    await user.click(screen.getByText("a.jsonl"));

    expect(screen.getByRole("button", { name: "Collapse error for 2 files" })).toBeInTheDocument();
    expect(screen.getByText("b.jsonl")).toBeInTheDocument();
  });
});

describe("VirtualizedImportIssuesTable keyboard", () => {
  it("moves between rows with the arrow keys and expands the focused row with Enter", async () => {
    const user = userEvent.setup();
    render(
      <VirtualizedImportIssuesTable
        issues={[
          issue({ item: "a.jsonl", reason: "could not read" }),
          issue({ item: "b.jsonl", reason: "HTTP 500 from server" }),
        ]}
      />,
    );

    // The table is one stop for Tab, on its first row.
    await user.tab();
    expect(screen.getByRole("row", { name: "a.jsonl" })).toHaveFocus();

    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("row", { name: "b.jsonl" })).toHaveFocus();

    await user.keyboard("{Enter}");
    expect(screen.getByRole("button", { name: "Collapse error for b.jsonl" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(screen.getByRole("button", { name: "Expand error for a.jsonl" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });
});

describe("tableViewportHeight", () => {
  it("grows a one-group viewport to fit the expanded reason and file list", () => {
    const reason = "source mismatch";
    const fileCount = 681;
    const expanded = estimateExpandedHeight(reason, fileCount);
    const viewport = tableViewportHeight(1, { reason, fileCount });

    expect(tableViewportHeight(1, null)).toBe(56);
    expect(viewport).toBeGreaterThanOrEqual(expanded);
    expect(viewport).toBe(expanded);
  });
});

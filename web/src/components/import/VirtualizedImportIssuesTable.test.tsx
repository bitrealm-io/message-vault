/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ImportIssue } from "./ImportSummaryPanel";
import { estimateExpandedHeight, tableViewportHeight } from "./importIssuesTableLayout";
import VirtualizedImportIssuesTable from "./VirtualizedImportIssuesTable";

vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getVirtualItems: () =>
      Array.from({ length: count }, (_, index) => ({
        index,
        key: index,
        start: index * 56,
        size: 56,
        end: (index + 1) * 56,
      })),
    getTotalSize: () => count * 56,
    measure: () => {},
    measureElement: () => {},
  }),
}));

afterEach(() => {
  cleanup();
});

function issue(partial: Partial<ImportIssue> & Pick<ImportIssue, "item" | "reason">): ImportIssue {
  return {
    kind: "error",
    step: "upload",
    ...partial,
  };
}

describe("VirtualizedImportIssuesTable", () => {
  it("names the Stage an error happened in, not the internal step", () => {
    render(
      <VirtualizedImportIssuesTable
        issues={[
          issue({ item: "a.jsonl", reason: "could not read", step: "parse" }),
          issue({ item: "b.jsonl", reason: "HTTP 500 from vault", step: "upload" }),
          issue({ item: "c.jsonl", reason: "new kind of step", step: "reindex" }),
        ]}
      />,
    );
    expect(screen.getByRole("columnheader", { name: "Stage" })).toBeInTheDocument();
    expect(screen.getByText("Staging")).toBeInTheDocument();
    expect(screen.getByText("Upload")).toBeInTheDocument();
    // A step this build does not know shows as it arrived.
    expect(screen.getByText("reindex")).toBeInTheDocument();
  });

  it("shows the filename for a unique issue", () => {
    render(
      <VirtualizedImportIssuesTable
        issues={[issue({ item: "chat.jsonl", reason: "HTTP 500 from vault" })]}
      />,
    );
    expect(screen.getByRole("table", { name: "Import errors" })).toHaveAttribute(
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
    expect(screen.getByRole("table", { name: "Import errors" })).toHaveAttribute(
      "aria-rowcount",
      "2",
    );
    expect(screen.getByText("3 files")).toBeInTheDocument();
    expect(screen.queryByText("a.jsonl")).not.toBeInTheDocument();

    await user.click(screen.getByRole("row", { name: /Expand error for 3 files/ }));

    expect(screen.getByRole("row", { name: /Collapse error for 3 files/ })).toBeInTheDocument();
    expect(screen.getByText("a.jsonl")).toBeInTheDocument();
    expect(screen.getByText("b.jsonl")).toBeInTheDocument();
    expect(screen.getByText("c.jsonl")).toBeInTheDocument();
    expect(screen.getByText("source mismatch")).toBeInTheDocument();
  });

  it("expands a unique row to the reason only", async () => {
    const user = userEvent.setup();
    render(
      <VirtualizedImportIssuesTable
        issues={[issue({ item: "chat.jsonl", reason: "HTTP 500 from vault" })]}
      />,
    );

    await user.click(screen.getByRole("row", { name: /Expand error for chat.jsonl/ }));

    expect(screen.getByText("HTTP 500 from vault")).toBeInTheDocument();
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

    await user.click(screen.getByRole("row", { name: /Expand error for 2 files/ }));
    await user.click(screen.getByText("a.jsonl"));

    expect(screen.getByRole("row", { name: /Collapse error for 2 files/ })).toBeInTheDocument();
    expect(screen.getByText("b.jsonl")).toBeInTheDocument();
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

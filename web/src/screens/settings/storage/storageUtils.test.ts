import { describe, expect, it } from "vitest";
import {
  describeExportScope,
  formatBytes,
  formatImportDate,
  type ImportDetailResponse,
  importStatusLabel,
  toImportSummaryView,
} from "./storageUtils";

function detail(partial: Partial<ImportDetailResponse> = {}): ImportDetailResponse {
  return {
    id: 1,
    source: "imessage-ios",
    tool: null,
    mode: "import",
    status: "completed",
    started_at: "2026-08-11T12:00:00Z",
    finished_at: "2026-08-11T12:01:00Z",
    message_count: 10,
    attachment_count: 0,
    bytes_uploaded: 0,
    duration_ms: 1000,
    parse_ms: null,
    attachments_ms: null,
    prepare_ms: null,
    upload_ms: null,
    summary: {},
    issues: [],
    ...partial,
  };
}

describe("formatBytes", () => {
  it("handles zero and non-finite", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(-1)).toBe("0 B");
    expect(formatBytes(Number.NaN)).toBe("0 B");
  });

  it("scales units", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MB");
  });
});

describe("importStatusLabel", () => {
  it("reads sentence-case for every status the server can return", () => {
    expect(importStatusLabel("running")).toBe("Running");
    expect(importStatusLabel("completed")).toBe("Completed");
    expect(importStatusLabel("completed_with_issues")).toBe("Completed with issues");
    expect(importStatusLabel("failed")).toBe("Failed");
    expect(importStatusLabel("canceled")).toBe("Canceled");
  });

  it("falls back to the raw string for anything unrecognized", () => {
    expect(importStatusLabel("mystery")).toBe("mystery");
  });
});

describe("formatImportDate", () => {
  it("returns em dash for empty", () => {
    expect(formatImportDate(null)).toBe("—");
    expect(formatImportDate(undefined)).toBe("—");
  });

  it("returns original string for invalid dates", () => {
    expect(formatImportDate("not-a-date")).toBe("not-a-date");
  });
});

describe("toImportSummaryView", () => {
  it("maps completed status and summary counts", () => {
    const view = toImportSummaryView(
      detail({
        summary: {
          files_total: 3,
          messages_inserted: 7,
          messages_deduped: 2,
        },
      }),
    );
    expect(view.status).toBe("completed");
    expect(view.filesTotal).toBe(3);
    expect(view.messagesInserted).toBe(7);
    expect(view.messagesDeduped).toBe(2);
  });

  it("treats unknown status as failed", () => {
    expect(toImportSummaryView(detail({ status: "exploded" })).status).toBe("failed");
  });

  it("passes completed_with_issues through", () => {
    expect(toImportSummaryView(detail({ status: "completed_with_issues" })).status).toBe(
      "completed_with_issues",
    );
  });

  it("falls back messagesInserted to message_count", () => {
    expect(toImportSummaryView(detail({ message_count: 42 })).messagesInserted).toBe(42);
  });

  it("sums stage timings when duration_ms is missing", () => {
    const view = toImportSummaryView(
      detail({
        duration_ms: null,
        parse_ms: 10,
        attachments_ms: 20,
        prepare_ms: 5,
        upload_ms: 30,
      }),
    );
    expect(view.durationMs).toBe(65);
    expect(view.attachmentsMs).toBe(20);
    expect(view.prepareMs).toBe(5);
  });
});

describe("describeExportScope", () => {
  it("names each scope form in one line", () => {
    expect(describeExportScope({ kind: "everything" })).toBe("Everything");
    expect(describeExportScope({ kind: "query", q: "from:me pizza" })).toBe(
      "Search: from:me pizza",
    );
    expect(
      describeExportScope({ kind: "selection", conversation_ids: [1, 2], message_ids: [9] }),
    ).toBe("Picked: 2 conversations, 1 message");
    expect(describeExportScope({ kind: "selection", conversation_ids: [4] })).toBe(
      "Picked: 1 conversation",
    );
  });
});

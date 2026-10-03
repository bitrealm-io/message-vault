import { describe, expect, it } from "vitest";
import {
  type AccountImportRun,
  describeExportRun,
  describeExportScope,
  formatBytes,
  formatImportDate,
  importStatusLabel,
  toImportSummaryView,
} from "./storageUtils";

function accountImportRun(partial: Partial<AccountImportRun> = {}): AccountImportRun {
  return {
    id: 1,
    source: "imessage-ios",
    tool: null,
    mode: "import",
    status: "completed",
    started_at: "2026-08-11T12:00:00Z",
    finished_at: "2026-08-11T12:01:00Z",
    dedupe: false,
    message_count: 10,
    contacts_new: 0,
    contacts_changed: 0,
    attachment_count: 0,
    bytes_uploaded: 0,
    duration_ms: 1000,
    parse_ms: null,
    attachments_ms: null,
    prepare_ms: null,
    upload_ms: null,
    form: null,
    source_fingerprint: null,
    source_identities: null,
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
    expect(importStatusLabel("cancelled")).toBe("Cancelled");
  });

  it("shows the raw word when a server newer than this build sends one", () => {
    expect(importStatusLabel("mystery" as never)).toBe("mystery");
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
      accountImportRun({
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
    expect(toImportSummaryView(accountImportRun({ status: "exploded" as never })).status).toBe(
      "failed",
    );
  });

  it("reads a cancelled Import Run as cancelled, not failed", () => {
    expect(toImportSummaryView(accountImportRun({ status: "cancelled" })).status).toBe("cancelled");
  });

  it("passes completed_with_issues through", () => {
    expect(toImportSummaryView(accountImportRun({ status: "completed_with_issues" })).status).toBe(
      "completed_with_issues",
    );
  });

  it("falls back messagesInserted to message_count", () => {
    expect(toImportSummaryView(accountImportRun({ message_count: 42 })).messagesInserted).toBe(42);
  });

  it("sums stage timings when duration_ms is missing", () => {
    const view = toImportSummaryView(
      accountImportRun({
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
    expect(describeExportScope({ kind: "query", list: "messages", q: "from:me pizza" })).toBe(
      "Messages found by: from:me pizza",
    );
    expect(describeExportScope({ kind: "query", list: "conversations", q: "messages:>100" })).toBe(
      "Conversations found by: messages:>100",
    );
    expect(
      describeExportScope({ kind: "selection", conversation_ids: [1, 2], message_ids: [9] }),
    ).toBe("Picked: 2 conversations, 1 message");
    expect(describeExportScope({ kind: "selection", conversation_ids: [4] })).toBe(
      "Picked: 1 conversation",
    );
  });
});

describe("describeExportRun", () => {
  const run = {
    id: 1,
    tool: null,
    status: "completed",
    started_at: "2026-08-11T12:00:00Z",
    message_count: 3,
    conversation_count: 1,
    attachment_count: 0,
    total_bytes: 0,
    messages_delivered: 3,
  } as const;

  it("tells the owner which form the scope took and nothing of what it asked for", () => {
    expect(describeExportRun({ ...run, scope_kind: "everything" })).toBe("Everything");
    expect(describeExportRun({ ...run, scope_kind: "query" })).toBe("A search");
    expect(describeExportRun({ ...run, scope_kind: "selection" })).toBe("Picked by hand");
  });

  it("tells the account its own search", () => {
    expect(
      describeExportRun({ ...run, scope: { kind: "query", list: "messages", q: "pizza" } }),
    ).toBe("Messages found by: pizza");
  });
});

describe("toImportSummaryView for the owner", () => {
  it("reads the counts the owner is given, and lists no issues", () => {
    const view = toImportSummaryView({
      id: 1,
      source: "imessage-ios",
      mode: "append",
      status: "completed_with_issues",
      started_at: "2026-08-11T12:00:00Z",
      message_count: 10,
      attachment_count: 0,
      bytes_uploaded: 0,
      counts: { messages_parsed: 12, messages_inserted: 10 },
      issue_count: 2,
      contacts_new: 0,
      contacts_changed: 0,
    });
    expect(view.messagesParsed).toBe(12);
    expect(view.messagesInserted).toBe(10);
    expect(view.issues).toEqual([]);
  });
});

import { describe, expect, it } from "vitest";
import type { ImportIssue } from "../../components/import/ImportSummaryPanel";
import type { PushFinishedReport } from "../../lib/tauri";
import {
  EMPTY_RUN_RECORD,
  filesSkippedOverRun,
  parseRunRecord,
  type RunPart,
  type RunRecord,
  recordToCarry,
  wholeRun,
} from "./runRecord";

function report(overrides: Partial<PushFinishedReport> = {}): PushFinishedReport {
  return {
    ok: true,
    cancelled: false,
    messages: 10,
    messages_attempted: 10,
    messages_inserted: 10,
    messages_deduped: 0,
    messages_failed: 0,
    assets_uploaded: 2,
    assets_bytes: 2_000,
    conversations_ok: 1,
    conversations_total: 1,
    conversations_failed: 0,
    conversations_skipped: 0,
    conversations_cancelled: 0,
    results: [],
    ...overrides,
  };
}

function part(overrides: Partial<RunPart> = {}): RunPart {
  return {
    issues: [],
    durationMs: 1_000.4,
    parseMs: null,
    attachmentsMs: null,
    prepareMs: null,
    uploadMs: null,
    report: null,
    ...overrides,
  };
}

describe("parseRunRecord", () => {
  it("reads a record back field by field", () => {
    const record = {
      issues: [{ kind: "skip", stage: "staging", item: "a.jpg", reason: "missing" }],
      parseMs: 10,
      uploadMs: 20,
      bytesUploaded: 30,
      messagesInserted: 40,
    };
    expect(parseRunRecord(record)).toEqual(record);
  });

  it("reads anything that is not a record as no earlier part", () => {
    expect(parseRunRecord(null)).toEqual(EMPTY_RUN_RECORD);
    expect(parseRunRecord("garbage")).toEqual(EMPTY_RUN_RECORD);
  });

  it("leaves out a malformed issue and a field that is not a number", () => {
    expect(
      parseRunRecord({
        issues: [{ kind: "skip" }, { kind: "error", stage: "upload", item: "x", reason: "y" }],
        uploadMs: "soon",
      }),
    ).toEqual({ issues: [{ kind: "error", stage: "upload", item: "x", reason: "y" }] });
  });
});

describe("wholeRun", () => {
  it("adds this part's times, bytes and Upload counts to the earlier parts'", () => {
    const carried: RunRecord = {
      issues: [{ kind: "skip", stage: "staging", item: "a.jpg", reason: "missing" }],
      durationMs: 5_000,
      parseMs: 100,
      attachmentsMs: 200,
      prepareMs: 300,
      uploadMs: 400,
      bytesUploaded: 1_000,
      filesParsed: 3,
      messagesParsed: 30,
      filesSucceeded: 2,
      messagesAttempted: 20,
      messagesInserted: 20,
      messagesDeduped: 0,
      attachmentsUploaded: 1,
    };
    const whole = wholeRun(
      carried,
      part({
        issues: [{ kind: "skip", stage: "upload", item: "c.jsonl:big.mov", reason: "too large" }],
        uploadMs: 600.6,
        report: report({ conversations_ok: 1, conversations_skipped: 2, conversations_total: 3 }),
      }),
    );

    expect(whole).toEqual({
      issues: [
        { kind: "skip", stage: "staging", item: "a.jpg", reason: "missing" },
        { kind: "skip", stage: "upload", item: "c.jsonl:big.mov", reason: "too large" },
      ],
      // Whole milliseconds, which is what the server stores.
      durationMs: 6_000,
      parseMs: 100,
      attachmentsMs: 200,
      prepareMs: 300,
      uploadMs: 1_001,
      bytesUploaded: 3_000,
      // A resumed Upload stages nothing, so Staging's counts are the earlier part's.
      filesParsed: 3,
      messagesParsed: 30,
      filesSucceeded: 3,
      messagesAttempted: 30,
      messagesInserted: 30,
      messagesDeduped: 0,
      attachmentsUploaded: 3,
    });
  });

  it("leaves out the skip rows of conversations an earlier part sent", () => {
    const whole = wholeRun(
      EMPTY_RUN_RECORD,
      part({
        issues: [
          { kind: "skip", stage: "upload", item: "a.jsonl", reason: "already imported or skipped" },
        ],
        report: report({
          results: [{ file: "a.jsonl", status: "skipped", messages: 0, attachments: 0 }],
        }),
      }),
    );
    expect(whole.issues).toEqual([]);
  });

  it("leaves a time no part measured unknown rather than zero", () => {
    expect(wholeRun(EMPTY_RUN_RECORD, part()).uploadMs).toBeUndefined();
  });
});

describe("recordToCarry", () => {
  it("keeps the attachment skips of a stopped Upload and drops what the resume reports again", () => {
    const issues: ImportIssue[] = [
      { kind: "skip", stage: "staging", item: "a.jpg", reason: "missing" },
      { kind: "skip", stage: "upload", item: "a.jsonl:big.mov", reason: "too large" },
      { kind: "error", stage: "upload", item: "b.jsonl", reason: "connection refused" },
      { kind: "skip", stage: "upload", item: "c.jsonl", reason: "the Upload was stopped" },
      { kind: "error", stage: "upload", item: "Import", reason: "the server went away" },
    ];
    const carried = recordToCarry(
      EMPTY_RUN_RECORD,
      part({
        issues,
        report: report({
          ok: false,
          results: [
            { file: "a.jsonl", status: "ok", messages: 1, attachments: 1 },
            { file: "b.jsonl", status: "failed", messages: 0, attachments: 0 },
            { file: "c.jsonl", status: "cancelled", messages: 0, attachments: 0 },
          ],
        }),
      }),
    );
    expect(carried.issues).toEqual([
      { kind: "skip", stage: "staging", item: "a.jpg", reason: "missing" },
      { kind: "skip", stage: "upload", item: "a.jsonl:big.mov", reason: "too large" },
    ]);
  });
});

describe("filesSkippedOverRun", () => {
  it("does not count as skipped the conversations an earlier part sent", () => {
    const carried = { issues: [], filesSucceeded: 200 };
    expect(filesSkippedOverRun(carried, report({ conversations_skipped: 205 }))).toBe(5);
    expect(filesSkippedOverRun(EMPTY_RUN_RECORD, report({ conversations_skipped: 4 }))).toBe(4);
  });
});

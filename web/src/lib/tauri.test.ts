import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PushFinishedReport } from "./tauri";
import {
  invokeDeleteStaging,
  invokeSummarizeStaging,
  invokeTranscodeStaging,
  parseTauriJobResult,
} from "./tauri";

const invoke = vi.fn();
const resolveStagingParent = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

vi.mock("./system-settings", () => ({
  resolveStagingParent: (...args: unknown[]) => resolveStagingParent(...args),
}));

function reportJson(overrides: Partial<PushFinishedReport> = {}): string {
  const report = {
    ok: true,
    messages: 10,
    messages_attempted: 10,
    messages_inserted: 10,
    messages_deduped: 0,
    messages_failed: 0,
    assets_uploaded: 1,
    assets_bytes: 100,
    conversations_ok: 5,
    conversations_total: 5,
    conversations_failed: 0,
    conversations_skipped: 0,
    results: [],
    ...overrides,
  };
  return JSON.stringify(report);
}

describe("parseTauriJobResult", () => {
  it("attaches a report that carries every field the verdict depends on", () => {
    const result = parseTauriJobResult(reportJson());
    expect(result.report).toBeDefined();
    expect(result.report?.conversations_failed).toBe(0);
    expect(result.report?.conversations_skipped).toBe(0);
  });

  // Regression for the last path back to the 2026-08-27 incident: a report
  // JSON blob missing conversations_failed/conversations_skipped must not
  // narrow to PushFinishedReport, or importOutcome computes
  // `undefined === 0` -> false and reports "completed" for a run where
  // nothing landed.
  it("does not attach a report missing conversations_failed", () => {
    const parsed: Record<string, unknown> = JSON.parse(reportJson());
    delete parsed.conversations_failed;
    const result = parseTauriJobResult(JSON.stringify(parsed));
    expect(result.report).toBeUndefined();
  });

  it("does not attach a report missing conversations_skipped", () => {
    const parsed: Record<string, unknown> = JSON.parse(reportJson());
    delete parsed.conversations_skipped;
    const result = parseTauriJobResult(JSON.stringify(parsed));
    expect(result.report).toBeUndefined();
  });

  it("still parses an extraction summary", () => {
    const result = parseTauriJobResult(
      JSON.stringify({ summary: "done", files_parsed: 3, messages_parsed: 20 }),
    );
    expect(result.extraction).toEqual({ files_parsed: 3, messages_parsed: 20 });
  });

  it("falls back to a plain summary for a non-JSON string", () => {
    const result = parseTauriJobResult("Extracted 10 messages.");
    expect(result).toEqual({ summary: "Extracted 10 messages." });
  });

  // transcode_staging's payload: TranscodeReport has no serde derive, so
  // staging.rs hand-builds the finished payload with these fields flat
  // alongside `summary`, snake_case, not nested under a `report` key. A
  // client that doesn't recognise this shape falls through to
  // `{ summary: <the raw JSON string> }`, which would render raw JSON
  // wherever the finished summary is displayed.
  it("recognizes the transcode-finished payload and returns typed counts, not raw JSON", () => {
    const summarySentence =
      "Converted 12 files; 2 will not be uploaded (still too large after conversion).";
    const payload = JSON.stringify({
      summary: summarySentence,
      converted: 12,
      skipped: 3,
      too_large: 2,
      failed: 1,
      missing: 0,
      repointed: 4,
      bytes_before: 900_000,
      bytes_after: 100_000,
    });

    const result = parseTauriJobResult(payload);

    expect(result.summary).toBe(summarySentence);
    expect(result.summary).not.toContain("{");
    expect(result.transcode).toEqual({
      converted: 12,
      skipped: 3,
      too_large: 2,
      failed: 1,
      missing: 0,
      repointed: 4,
      bytes_before: 900_000,
      bytes_after: 100_000,
    });
    expect(result.report).toBeUndefined();
    expect(result.extraction).toBeUndefined();
  });

  it("does not mistake a transcode payload missing a count for one it recognizes", () => {
    const parsed: Record<string, unknown> = JSON.parse(
      JSON.stringify({
        summary: "Converted 1 file.",
        converted: 1,
        skipped: 0,
        too_large: 0,
        failed: 0,
        missing: 0,
        repointed: 0,
        bytes_before: 10,
        // bytes_after omitted
      }),
    );
    const result = parseTauriJobResult(JSON.stringify(parsed));
    expect(result.transcode).toBeUndefined();
  });
});

describe("staging command wrappers resolve their own staging root", () => {
  beforeEach(async () => {
    invoke.mockReset();
    resolveStagingParent.mockReset();
    invoke.mockResolvedValue(undefined);
    resolveStagingParent.mockResolvedValue("/home/sam/message-crate");
  });

  it("invokeSummarizeStaging resolves the root itself rather than taking one from the caller", async () => {
    await invokeSummarizeStaging({
      staging_dir: "/home/sam/message-crate/staging-run",
      asset_max_bytes: 104857600,
    });

    expect(resolveStagingParent).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("summarize_staging", {
      args: expect.objectContaining({
        stagingDir: "/home/sam/message-crate/staging-run",
        stagingRoot: "/home/sam/message-crate",
        // The command takes the run's limit; it has no number of its own.
        assetMaxBytes: 104857600,
      }),
    });
  });

  it("invokeTranscodeStaging resolves the root itself rather than taking one from the caller", async () => {
    await invokeTranscodeStaging({
      staging_dir: "/home/sam/message-crate/staging-run",
      attachment_media: "convert",
      asset_max_bytes: 104857600,
    });

    expect(resolveStagingParent).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("transcode_staging", {
      args: expect.objectContaining({
        stagingDir: "/home/sam/message-crate/staging-run",
        stagingRoot: "/home/sam/message-crate",
        attachmentMedia: "convert",
        assetMaxBytes: 104857600,
      }),
    });
  });

  it("invokeDeleteStaging resolves the root itself rather than taking one from the caller", async () => {
    await invokeDeleteStaging({ staging_dir: "/home/sam/message-crate/staging-run" });

    expect(resolveStagingParent).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("delete_staging", {
      args: {
        stagingDir: "/home/sam/message-crate/staging-run",
        stagingRoot: "/home/sam/message-crate",
      },
    });
  });

  it("rejects rather than calling through when the staging root cannot be resolved", async () => {
    resolveStagingParent.mockResolvedValue("");

    await expect(
      invokeSummarizeStaging({
        staging_dir: "/home/sam/message-crate/staging-run",
        asset_max_bytes: 104857600,
      }),
    ).rejects.toThrow(/staging directory/i);
    expect(invoke).not.toHaveBeenCalled();
  });
});

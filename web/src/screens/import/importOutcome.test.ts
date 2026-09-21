import { describe, expect, it } from "vitest";
import type {
  AttachmentForecast,
  PushFinishedReport,
  SizeVerdict,
  StagingSummary,
} from "../../lib/tauri";
import { importOutcome, stableStem } from "./importOutcome";

function report(overrides: Partial<PushFinishedReport> = {}): PushFinishedReport {
  return {
    ok: true,
    messages: 100,
    messages_attempted: 100,
    messages_inserted: 100,
    messages_deduped: 0,
    messages_failed: 0,
    assets_uploaded: 5,
    assets_bytes: 1_000,
    conversations_ok: 10,
    conversations_total: 10,
    conversations_failed: 0,
    conversations_skipped: 0,
    results: [],
    ...overrides,
  };
}

/** `n` push issues for attachments the plan flagged as too big to upload —
 * the `"{conversationFile}:{relativePath}"` shape `vault-push` actually
 * emits (`AttachmentSkipIssue` in `crates/cli/vault-push/src/run.rs`), not
 * a bare path. */
function tooLargeIssues(n: number): { kind: string; step: string; item: string; reason: string }[] {
  return Array.from({ length: n }, (_, i) => ({
    kind: "skip",
    step: "upload",
    item: `conversation.jsonl:attachments/2024-01-15-toolarge${i}.jpg`,
    reason: "attachment is 200000000 bytes (200 MiB), over the configured asset max of 100 MiB",
  }));
}

/** An approved `StagingSummary` flagging `tooLarge` attachments as
 * `probably_too_big`, at the paths `tooLargeIssues` uses by default. */
function approvedPlan(counts: { tooLarge?: number } = {}): StagingSummary {
  const tooLarge = counts.tooLarge ?? 0;
  const forecasts: AttachmentForecast[] = Array.from({ length: tooLarge }, (_, i) => ({
    path: `attachments/2024-01-15-toolarge${i}.jpg`,
    name: `IMG_${i}.jpg`,
    sizeBytes: 200_000_000,
    estimateBytes: 200_000_000,
    verdict: "probably_too_big" as SizeVerdict,
  }));
  return {
    conversations: 1,
    messages: 1,
    contactIdentifiers: [],
    outgoingHandles: [],
    attachments: forecasts.length,
    attachmentBytes: 0,
    verdictCounts: {
      fitsAsIs: 0,
      likelyFits: 0,
      mayGrow: 0,
      probablyTooBig: tooLarge,
      cannotProcess: 0,
    },
    forecasts,
    assetMaxBytes: 50 * 1024 * 1024,
  };
}

describe("stableStem", () => {
  it("reads a converted file as the same attachment it was staged as", () => {
    expect(stableStem("attachments/2024-01-15-9f2a3b4c.heic")).toBe("2024-01-15-9f2a3b4c");
    expect(stableStem("attachments/2024-01-15-9f2a3b4c-mv.jpg")).toBe("2024-01-15-9f2a3b4c");
  });

  it("keeps a name with no extension whole", () => {
    expect(stableStem("attachments/README")).toBe("README");
  });
});

describe("importOutcome", () => {
  it("is completed for a clean run", () => {
    expect(importOutcome({ report: report(), threw: false, issues: [] })).toBe("completed");
  });

  it("is failed when the job threw, whatever the report says", () => {
    expect(importOutcome({ report: report(), threw: true, issues: [] })).toBe("failed");
  });

  it("is failed when there is no report at all", () => {
    expect(importOutcome({ report: undefined, threw: false, issues: [] })).toBe("failed");
  });

  it("is failed when every conversation failed and nothing landed (2026-08-27 shape)", () => {
    const r = report({
      ok: false,
      conversations_total: 681,
      conversations_ok: 0,
      conversations_failed: 681,
      conversations_skipped: 0,
      messages_inserted: 0,
      messages_failed: 8_000,
    });
    expect(importOutcome({ report: r, threw: false, issues: [] })).toBe("failed");
  });

  it("is completed when a re-push dedupes everything to skips", () => {
    const r = report({
      conversations_total: 10,
      conversations_ok: 0,
      conversations_failed: 0,
      conversations_skipped: 10,
      messages_inserted: 0,
    });
    expect(importOutcome({ report: r, threw: false, issues: [] })).toBe("completed");
  });

  it("is completed_with_issues when some conversations failed but others landed", () => {
    const r = report({ ok: false, conversations_ok: 8, conversations_failed: 2 });
    expect(importOutcome({ report: r, threw: false, issues: [] })).toBe("completed_with_issues");
  });

  it("is completed_with_issues when messages failed inside ok conversations", () => {
    const r = report({ messages_failed: 3 });
    expect(importOutcome({ report: r, threw: false, issues: [] })).toBe("completed_with_issues");
  });

  it("is completed_with_issues when the run recorded an issue", () => {
    expect(
      importOutcome({
        report: report(),
        threw: false,
        issues: [{ kind: "skip", step: "upload", item: "attachments/a.jpg", reason: "not found" }],
      }),
    ).toBe("completed_with_issues");
  });
});

describe("importOutcome against an approved plan", () => {
  it("an approved omission is not an issue", () => {
    // The user saw "12 attachments too big" at the gate and said go.
    // Reporting that back as a problem makes a normal import look like a
    // failure.
    const outcome = importOutcome({
      report: report({ conversations_ok: 10, messages_inserted: 500 }),
      threw: false,
      issues: tooLargeIssues(12),
      approved: approvedPlan({ tooLarge: 12 }),
    });
    expect(outcome).toBe("completed");
  });

  it("one omission nobody forecast is an issue", () => {
    const outcome = importOutcome({
      report: report({ conversations_ok: 10, messages_inserted: 500 }),
      threw: false,
      issues: tooLargeIssues(1),
      approved: approvedPlan({ tooLarge: 0 }),
    });
    expect(outcome).toBe("completed_with_issues");
  });

  it("zero conversations is a failure however clean the issue list is", () => {
    // Decision 21's floor, unchanged by this task: conversations_total > 0
    // with nothing ok and nothing skipped means nothing landed at all.
    const outcome = importOutcome({
      report: report({ conversations_ok: 0, messages_inserted: 0 }),
      threw: false,
      issues: [],
      approved: approvedPlan({ tooLarge: 0 }),
    });
    expect(outcome).toBe("failed");
  });

  it("behaves exactly as before when there is no approved plan", () => {
    const args = {
      report: report({ conversations_ok: 10, messages_inserted: 500 }),
      threw: false,
      issues: tooLargeIssues(3),
    };
    expect(importOutcome(args)).toBe("completed_with_issues");
    expect(importOutcome({ ...args, approved: undefined })).toBe("completed_with_issues");
  });

  it("matches an approved row by its plain name, not just its path", () => {
    const approved: StagingSummary = {
      ...approvedPlan(),
      forecasts: [
        {
          path: "attachments/2024-01-15-9f2a3b4c.heic",
          name: "special-name.heic",
          sizeBytes: 200_000_000,
          estimateBytes: 200_000_000,
          verdict: "cannot_process",
        },
      ],
    };
    const outcome = importOutcome({
      report: report({ conversations_ok: 10, messages_inserted: 500 }),
      threw: false,
      // The issue's item is the bare forecast name, no conversation-file
      // prefix and no path.
      issues: [{ kind: "skip", step: "upload", item: "special-name.heic", reason: "too large" }],
      approved,
    });
    expect(outcome).toBe("completed");
  });

  it("matches a converted -mv path back to its pre-conversion stem", () => {
    const approved: StagingSummary = {
      ...approvedPlan(),
      forecasts: [
        {
          path: "attachments/2024-01-15-9f2a3b4c.heic",
          name: "IMG_0001.heic",
          sizeBytes: 200_000_000,
          estimateBytes: 200_000_000,
          verdict: "probably_too_big",
        },
      ],
    };
    const outcome = importOutcome({
      report: report({ conversations_ok: 10, messages_inserted: 500 }),
      threw: false,
      issues: [
        {
          kind: "skip",
          step: "upload",
          // The committed derivative after the media pass: same stem,
          // "-mv" suffix, new extension.
          item: "conversation.jsonl:attachments/2024-01-15-9f2a3b4c-mv.jpg",
          reason: "attachment is 200000000 bytes, over the configured asset max",
        },
      ],
      approved,
    });
    expect(outcome).toBe("completed");
  });

  it("an issue for a file the plan never flagged is unexpected, even with a plan present", () => {
    const outcome = importOutcome({
      report: report({ conversations_ok: 10, messages_inserted: 500 }),
      threw: false,
      issues: [
        {
          kind: "skip",
          step: "upload",
          item: "conversation.jsonl:attachments/2024-02-02-unrelated.jpg",
          reason: "attachment file not found on disk",
        },
      ],
      approved: approvedPlan({ tooLarge: 12 }),
    });
    expect(outcome).toBe("completed_with_issues");
  });

  it("an error is never excused by the plan, even for a file the plan flagged", () => {
    const approved: StagingSummary = {
      ...approvedPlan(),
      forecasts: [
        {
          path: "attachments/2024-01-15-toolarge0.jpg",
          name: "IMG_0.jpg",
          sizeBytes: 200_000_000,
          estimateBytes: 200_000_000,
          verdict: "probably_too_big",
        },
      ],
    };
    const outcome = importOutcome({
      report: report({ conversations_ok: 10, messages_inserted: 500 }),
      threw: false,
      issues: [
        {
          kind: "error",
          step: "upload",
          item: "conversation.jsonl:attachments/2024-01-15-toolarge0.jpg",
          reason: "upload failed",
        },
      ],
      approved,
    });
    expect(outcome).toBe("completed_with_issues");
  });

  it("a fits_as_is forecast row does not excuse a skip for that file", () => {
    // Only the verdicts that predict an omission (`probably_too_big`,
    // `cannot_process`) count as approved — a row the plan expected to land
    // is not the kind of "expected omission" decision 15 describes.
    const approved: StagingSummary = {
      ...approvedPlan(),
      forecasts: [
        {
          path: "attachments/2024-01-15-fine.jpg",
          name: "IMG_fine.jpg",
          sizeBytes: 1_000,
          estimateBytes: 1_000,
          verdict: "fits_as_is",
        },
      ],
    };
    const outcome = importOutcome({
      report: report({ conversations_ok: 10, messages_inserted: 500 }),
      threw: false,
      issues: [
        {
          kind: "skip",
          step: "upload",
          item: "conversation.jsonl:attachments/2024-01-15-fine.jpg",
          reason: "attachment file not found on disk",
        },
      ],
      approved,
    });
    expect(outcome).toBe("completed_with_issues");
  });
});

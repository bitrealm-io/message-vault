import { describe, expect, it } from "vitest";
import { groupImportIssues } from "./groupImportIssues";
import type { ImportIssue } from "./ImportSummaryPanel";

function issue(partial: Partial<ImportIssue> & Pick<ImportIssue, "item" | "reason">): ImportIssue {
  return {
    kind: "error",
    stage: "upload",
    ...partial,
  };
}

describe("groupImportIssues", () => {
  it("returns no groups for an empty list", () => {
    expect(groupImportIssues([])).toEqual([]);
  });

  it("collapses three identical reasons into one group with three items", () => {
    const groups = groupImportIssues([
      issue({ item: "a.jsonl", reason: "source mismatch" }),
      issue({ item: "b.jsonl", reason: "source mismatch" }),
      issue({ item: "c.jsonl", reason: "source mismatch" }),
    ]);
    expect(groups).toEqual([
      {
        kind: "error",
        stage: "upload",
        reason: "source mismatch",
        items: ["a.jsonl", "b.jsonl", "c.jsonl"],
      },
    ]);
  });

  it("keeps two different reasons as two groups", () => {
    const groups = groupImportIssues([
      issue({ item: "a.jsonl", reason: "source mismatch" }),
      issue({ item: "b.jsonl", reason: "HTTP 500 from server" }),
    ]);
    expect(groups).toHaveLength(2);
    expect(groups[0]?.reason).toBe("source mismatch");
    expect(groups[0]?.items).toEqual(["a.jsonl"]);
    expect(groups[1]?.reason).toBe("HTTP 500 from server");
    expect(groups[1]?.items).toEqual(["b.jsonl"]);
  });

  it("keeps error and skip with the same stage and reason as two groups", () => {
    const groups = groupImportIssues([
      issue({ kind: "error", item: "a.jsonl", reason: "source mismatch" }),
      issue({ kind: "skip", item: "b.jsonl", reason: "source mismatch" }),
    ]);
    expect(groups).toHaveLength(2);
    expect(groups[0]?.kind).toBe("error");
    expect(groups[0]?.items).toEqual(["a.jsonl"]);
    expect(groups[1]?.kind).toBe("skip");
    expect(groups[1]?.items).toEqual(["b.jsonl"]);
  });

  it("keeps first-seen group order and filename order", () => {
    const groups = groupImportIssues([
      issue({ stage: "upload", item: "b.jsonl", reason: "shared" }),
      issue({ stage: "staging", item: "early.jsonl", reason: "unique parse" }),
      issue({ stage: "upload", item: "c.jsonl", reason: "shared" }),
    ]);
    expect(groups.map((group) => group.reason)).toEqual(["shared", "unique parse"]);
    expect(groups[0]?.items).toEqual(["b.jsonl", "c.jsonl"]);
    expect(groups[1]?.items).toEqual(["early.jsonl"]);
  });

  it("keeps a duplicate filename when the stored list repeats it", () => {
    const groups = groupImportIssues([
      issue({ item: "same.jsonl", reason: "shared" }),
      issue({ item: "same.jsonl", reason: "shared" }),
    ]);
    expect(groups).toHaveLength(1);
    expect(groups[0]?.items).toEqual(["same.jsonl", "same.jsonl"]);
  });
});

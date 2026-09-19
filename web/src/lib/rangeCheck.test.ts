import { describe, expect, it } from "vitest";
import { applyCheckedRange } from "./rangeCheck";

const ids = ["a", "b", "c", "d", "e", "f", "g"];

function sorted(set: Set<string>): string[] {
  return [...set].sort();
}

describe("applyCheckedRange", () => {
  it("checks from the last clicked row down to the shift-click", () => {
    const next = applyCheckedRange(ids, new Set(["b"]), "b", "e", true);
    expect(sorted(next)).toEqual(["b", "c", "d", "e"]);
  });

  it("checks from the last clicked row up to the shift-click", () => {
    const next = applyCheckedRange(ids, new Set(["f"]), "f", "c", true);
    expect(sorted(next)).toEqual(["c", "d", "e", "f"]);
  });

  it("leaves checked rows outside the range alone", () => {
    const next = applyCheckedRange(ids, new Set(["a", "e"]), "e", "g", true);
    expect(sorted(next)).toEqual(["a", "e", "f", "g"]);
  });

  it("unchecks the range when the shift-clicked row lands unchecked", () => {
    const next = applyCheckedRange(ids, new Set(["b", "c", "d", "e", "f"]), "e", "c", false);
    expect(sorted(next)).toEqual(["b", "f"]);
  });

  it("changes only the clicked row when there is no anchor", () => {
    expect(sorted(applyCheckedRange(ids, new Set(["a"]), null, "c", true))).toEqual(["a", "c"]);
  });

  it("changes only the clicked row when the anchor is no longer in the list", () => {
    expect(sorted(applyCheckedRange(ids, new Set(["zz"]), "zz", "c", true))).toEqual(["c", "zz"]);
  });

  it("returns the set unchanged when the clicked id is not in the list", () => {
    expect(sorted(applyCheckedRange(ids, new Set(["c"]), "c", "zz", true))).toEqual(["c"]);
  });
});

import { describe, expect, it } from "vitest";
import { extendCheckedRange } from "./rangeCheck";

const ids = ["a", "b", "c", "d", "e", "f", "g"];

function sorted(set: Set<string>): string[] {
  return [...set].sort();
}

describe("extendCheckedRange", () => {
  it("checks from the bottommost checked row up to a shift-click above the topmost", () => {
    const next = extendCheckedRange(ids, new Set(["d", "f"]), "b");
    expect(sorted(next)).toEqual(["b", "c", "d", "e", "f"]);
  });

  it("checks from the topmost checked row down to a shift-click below the bottommost", () => {
    const next = extendCheckedRange(ids, new Set(["b", "d"]), "g");
    expect(sorted(next)).toEqual(["b", "c", "d", "e", "f", "g"]);
  });

  it("fills the gaps when the shift-click lands between checked rows", () => {
    const next = extendCheckedRange(ids, new Set(["b", "f"]), "d");
    expect(sorted(next)).toEqual(["b", "c", "d", "e", "f"]);
  });

  it("checks only the clicked row when nothing is checked yet", () => {
    expect(sorted(extendCheckedRange(ids, new Set(), "c"))).toEqual(["c"]);
  });

  it("keeps checked ids that are not in the list", () => {
    const next = extendCheckedRange(ids, new Set(["zz", "c"]), "a");
    expect(sorted(next)).toEqual(["a", "b", "c", "zz"]);
  });

  it("returns the set unchanged when the clicked id is not in the list", () => {
    expect(sorted(extendCheckedRange(ids, new Set(["c"]), "zz"))).toEqual(["c"]);
  });
});

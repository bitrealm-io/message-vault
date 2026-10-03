import { describe, expect, it } from "vitest";
import { matchRanges, snippet } from "./messageMatch";

const term = (text: string, prefix = false) => ({ text, prefix });

/** The matched pieces of `text`, for reading a test at a glance. */
function matched(text: string, terms: ReturnType<typeof term>[]): string[] {
  return matchRanges(text, terms).map(([a, b]) => text.slice(a, b));
}

describe("matchRanges", () => {
  it("matches whole words, ignoring case", () => {
    expect(matched("The Dentist called the dentistry", [term("dentist")])).toEqual(["Dentist"]);
  });

  it("matches a prefix at the start of a word, to the end of that word", () => {
    expect(matched("an avocado and guacamole", [term("avoc", true)])).toEqual(["avocado"]);
    expect(matched("guacamole", [term("cam", true)])).toEqual([]);
  });

  it("matches across accents, as the full-text index does", () => {
    expect(matched("Café au lait", [term("cafe")])).toEqual(["Café"]);
    expect(matched("cafe au lait", [term("café")])).toEqual(["cafe"]);
  });

  it("matches a phrase and a word with punctuation as words next to each other", () => {
    expect(matched("see you  there, ok", [term("you there")])).toEqual(["you  there"]);
    expect(matched("Mr O'Brien and o brien", [term("o'brien")])).toEqual(["O'Brien", "o brien"]);
  });

  it("matches every term and returns the ranges in order, merged where they meet", () => {
    expect(matched("photo of the dentist photo", [term("photo"), term("dentist")])).toEqual([
      "photo",
      "dentist",
      "photo",
    ]);
    expect(matched("new york", [term("new york"), term("york")])).toEqual(["new york"]);
  });

  it("matches nothing for a term of punctuation only", () => {
    expect(matched("!!! wow", [term("!!!")])).toEqual([]);
  });
});

describe("snippet", () => {
  it("keeps short text whole", () => {
    expect(snippet("the dentist called", [term("dentist")])).toEqual({
      text: "the dentist called",
      ranges: [[4, 11]],
    });
  });

  it("cuts long text at a word before the first match and marks the cut", () => {
    const text = `${"lorem ipsum dolor sit amet ".repeat(4)}then the dentist called again`;
    const out = snippet(text, [term("dentist")]);
    expect(out.text.startsWith("…")).toBe(true);
    expect(out.text).toContain("dentist called again");
    expect(out.text.length).toBeLessThan(text.length);
    const [[a, b]] = out.ranges;
    expect(out.text.slice(a, b)).toBe("dentist");
    // The cut lands on a word, not inside one.
    expect(out.text[1]).not.toBe(" ");
    expect(text).toContain(out.text.slice(1));
  });

  it("keeps the start of text with no match", () => {
    expect(snippet("nothing here", [term("dentist")])).toEqual({
      text: "nothing here",
      ranges: [],
    });
  });
});

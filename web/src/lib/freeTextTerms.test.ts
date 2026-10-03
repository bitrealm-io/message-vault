import { describe, expect, it } from "vitest";
import { freeTextTerms, hasFreeText } from "./freeTextTerms";

describe("freeTextTerms", () => {
  it("reads plain words and leaves field words out", () => {
    expect(freeTextTerms("from:Alice photo")).toEqual([{ text: "photo", prefix: false }]);
    expect(freeTextTerms('from:"Alice Smith" date:2024')).toEqual([]);
  });

  it("reads a quoted phrase as one term, with a doubled quote as one quote", () => {
    expect(freeTextTerms('"see you ""there"""')).toEqual([
      { text: 'see you "there"', prefix: false },
    ]);
  });

  it("reads a trailing star as a prefix", () => {
    expect(freeTextTerms("avoc*")).toEqual([{ text: "avoc", prefix: true }]);
  });

  it("leaves out a word behind a minus or not, and every word in a negated group", () => {
    expect(freeTextTerms("dentist -office")).toEqual([{ text: "dentist", prefix: false }]);
    expect(freeTextTerms("not dentist")).toEqual([]);
    expect(freeTextTerms("-(toast or guacamole) avocado")).toEqual([
      { text: "avocado", prefix: false },
    ]);
    expect(freeTextTerms("not (toast guacamole) avocado")).toEqual([
      { text: "avocado", prefix: false },
    ]);
  });

  it("reads the words on both sides of or and and, and not the operators", () => {
    expect(freeTextTerms("moved OR dentist and (toast)")).toEqual([
      { text: "moved", prefix: false },
      { text: "dentist", prefix: false },
      { text: "toast", prefix: false },
    ]);
  });

  it("reads a word with a colon that is not a field word as text", () => {
    expect(freeTextTerms("12:30 https://example.com")).toEqual([
      { text: "12:30", prefix: false },
      { text: "https://example.com", prefix: false },
    ]);
  });
});

describe("hasFreeText", () => {
  it("is true only when a positive free-text word is left", () => {
    expect(hasFreeText("from:Alice photo")).toBe(true);
    expect(hasFreeText("from:Alice date:2024")).toBe(false);
    expect(hasFreeText("-photo")).toBe(false);
    expect(hasFreeText("")).toBe(false);
  });
});

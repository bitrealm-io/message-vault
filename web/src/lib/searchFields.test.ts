import { describe, expect, it } from "vitest";
import { hasFieldToken, stripFieldTokens } from "./searchFields";

describe("hasFieldToken", () => {
  it("is true for any word: token, negated or not", () => {
    expect(hasFieldToken("group:Family")).toBe(true);
    expect(hasFieldToken("ana -tag:Work")).toBe(true);
    expect(hasFieldToken('title:"book club"')).toBe(true);
  });
  it("sees a token that a bracket opens", () => {
    expect(hasFieldToken("(kind:group or kind:direct)")).toBe(true);
  });
  it("is false for plain words, phrases, and colons inside a phrase", () => {
    expect(hasFieldToken("ana")).toBe(false);
    expect(hasFieldToken('"re: dinner"')).toBe(false);
    expect(hasFieldToken("http://example.com")).toBe(false);
    expect(hasFieldToken("")).toBe(false);
  });
});

describe("stripFieldTokens", () => {
  it("keeps the words and drops the tokens", () => {
    expect(stripFieldTokens("ana group:Family")).toBe("ana");
    expect(stripFieldTokens('identity:"+1 555" bo -tag:x')).toBe("bo");
    expect(stripFieldTokens("just words")).toBe("just words");
  });
  it("drops a token a bracket opens too", () => {
    expect(stripFieldTokens("(kind:group or kind:direct)")).toBe("or");
  });
});

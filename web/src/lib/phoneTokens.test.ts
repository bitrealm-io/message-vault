import { describe, expect, it } from "vitest";
import {
  commitPhoneTokens,
  normalizePhoneDigits,
  ownerPhonesMatchProfile,
  ownerPhonesNeedMismatchAck,
  phonesMatch,
  removePhoneToken,
  splitPhoneTokenInput,
} from "./phoneTokens";

describe("splitPhoneTokenInput", () => {
  it("splits on commas and keeps spaces inside a number", () => {
    expect(splitPhoneTokenInput(" +1 555-1111, +15552222  ")).toEqual(["+1 555-1111", "+15552222"]);
  });

  it("returns empty for blank input", () => {
    expect(splitPhoneTokenInput("  ,  ")).toEqual([]);
  });
});

describe("commitPhoneTokens", () => {
  it("appends new phones and skips duplicates", () => {
    expect(commitPhoneTokens(["+15551111"], "+15551111, +15552222")).toEqual([
      "+15551111",
      "+15552222",
    ]);
  });
});

describe("removePhoneToken", () => {
  it("removes the first matching value", () => {
    expect(removePhoneToken(["a", "b", "a"], "a")).toEqual(["b", "a"]);
  });
});

describe("normalizePhoneDigits", () => {
  it("strips non-digits", () => {
    expect(normalizePhoneDigits("+1 555-555-0119")).toBe("15555550119");
  });
});

describe("phonesMatch", () => {
  it("matches a 10-digit US number to E.164 with country code", () => {
    expect(phonesMatch("9415550100", "+19415550100")).toBe(true);
  });

  it("matches formatted US national to E.164", () => {
    expect(phonesMatch("(941) 555-0100", "+19415550100")).toBe(true);
  });

  it("does not match a different number", () => {
    expect(phonesMatch("9415550101", "+19415550100")).toBe(false);
  });

  it("does not treat a short code as a suffix of a longer number", () => {
    expect(phonesMatch("60605", "+19415550100")).toBe(false);
  });
});

describe("ownerPhonesMatchProfile", () => {
  it("matches when digits overlap despite formatting", () => {
    expect(ownerPhonesMatchProfile(["+1 555-555-0119"], ["15555550119"])).toBe(true);
  });

  it("matches a 10-digit owner phone to an E.164 profile phone", () => {
    expect(ownerPhonesMatchProfile(["9415550100"], ["+19415550100"])).toBe(true);
  });

  it("returns false when no overlap", () => {
    expect(ownerPhonesMatchProfile(["+15551111"], ["+15552222"])).toBe(false);
  });

  it("returns false when profile has no phones", () => {
    expect(ownerPhonesMatchProfile(["+15551111"], [])).toBe(false);
  });

  it("returns false when owner list is empty", () => {
    expect(ownerPhonesMatchProfile([], ["+15551111"])).toBe(false);
  });
});

describe("ownerPhonesNeedMismatchAck", () => {
  const readyOk = { ready: true, fetchFailed: false };

  it("returns false until profile is ready", () => {
    expect(ownerPhonesNeedMismatchAck(["+1"], [], { ready: false, fetchFailed: false })).toBe(
      false,
    );
  });

  it("returns false when profile fetch failed", () => {
    expect(ownerPhonesNeedMismatchAck(["+1"], [], { ready: true, fetchFailed: true })).toBe(false);
  });

  it("returns true for empty profile after a successful load", () => {
    expect(ownerPhonesNeedMismatchAck(["+15551111"], [], readyOk)).toBe(true);
    expect(ownerPhonesNeedMismatchAck([], [], readyOk)).toBe(true);
  });

  it("returns true when owner phones do not match profile", () => {
    expect(ownerPhonesNeedMismatchAck(["+15551111"], ["+15552222"], readyOk)).toBe(true);
  });

  it("returns false when owner phones match profile", () => {
    expect(ownerPhonesNeedMismatchAck(["+1 555-1111"], ["15551111"], readyOk)).toBe(false);
  });

  it("returns false when profile has phones but owner list is still empty", () => {
    expect(ownerPhonesNeedMismatchAck([], ["+15551111"], readyOk)).toBe(false);
  });
});

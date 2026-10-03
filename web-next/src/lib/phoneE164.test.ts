import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { formatPhoneDisplay, toPhoneE164 } from "./phoneE164";

describe("toPhoneE164 guarded policy", () => {
  it("normalizes unambiguous values to E.164", () => {
    assert.equal(toPhoneE164("5555550100"), "+15555550100");
    assert.equal(toPhoneE164("15555550100"), "+15555550100");
    assert.equal(toPhoneE164("+15555550100"), "+15555550100");
    assert.equal(toPhoneE164("+44 20 7183 8750"), "+442071838750");
    assert.equal(toPhoneE164("+1-542-555-0100"), "+15425550100");
  });

  it("returns null for ambiguous values instead of fabricating +0…", () => {
    assert.equal(toPhoneE164("020 7946 0000"), null);
    assert.equal(toPhoneE164("02079460000"), null);
    assert.equal(toPhoneE164("442079460000"), null);
    assert.equal(toPhoneE164("+02079460000"), null);
    assert.equal(toPhoneE164("7535"), null);
    assert.equal(toPhoneE164(""), null);
  });
});

describe("formatPhoneDisplay", () => {
  it("formats US E.164 with international spacing", () => {
    assert.equal(formatPhoneDisplay("+19415550100"), "+1 941 555 0100");
  });

  it("formats UK E.164 with international spacing", () => {
    assert.equal(formatPhoneDisplay("+447911123456"), "+44 7911 123456");
  });

  it("leaves emails unchanged", () => {
    assert.equal(
      formatPhoneDisplay("annette@example.com"),
      "annette@example.com",
    );
  });

  it("returns the original string when unparseable", () => {
    assert.equal(formatPhoneDisplay("not-a-phone"), "not-a-phone");
    assert.equal(formatPhoneDisplay(""), "");
    assert.equal(formatPhoneDisplay(null), "");
  });
});

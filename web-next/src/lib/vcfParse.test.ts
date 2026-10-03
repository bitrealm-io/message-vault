import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  cardToDraft,
  parseVcfText,
  splitCategories,
} from "./vcfParse";

const DUPLICATE_PHONE_VCF = `BEGIN:VCARD
VERSION:3.0
FN:Ada Augusta Lovelace
N:Lovelace;Ada;Augusta;;
TEL:+15555550119
CATEGORIES:Family
END:VCARD
BEGIN:VCARD
VERSION:3.0
FN:Ada Duplicate
N:Duplicate;Ada;;;
TEL:+15555550119
TEL:+15555550178
CATEGORIES:Work
END:VCARD
BEGIN:VCARD
VERSION:3.0
FN:Mononym
N:;Mononym;;;
TEL:+15555550176
CATEGORIES:Friends
END:VCARD
`;

describe("splitCategories", () => {
  it("splits on commas and unescapes", () => {
    assert.deepEqual(splitCategories(String.raw`Family,Work\,Inc,Friends`), [
      "Family",
      "Work,Inc",
      "Friends",
    ]);
  });

  it("trims empty tokens", () => {
    assert.deepEqual(splitCategories("  Family  ,  "), ["Family"]);
    assert.deepEqual(splitCategories(""), []);
  });
});

describe("parseVcfText CATEGORIES", () => {
  it("collects repeated CATEGORIES and dedupes case-insensitively", () => {
    const cards = parseVcfText(`BEGIN:VCARD
VERSION:3.0
FN:Ada Lovelace
N:Lovelace;Ada;;;
TEL:+15555550119
CATEGORIES:Family,Friends
CATEGORIES:Work
CATEGORIES:family
END:VCARD
`);
    assert.equal(cards.length, 1);
    assert.deepEqual(cards[0]!.categories, ["Family", "Friends", "Work"]);
  });

  it("merges bracket FN tags with CATEGORIES in cardToDraft", () => {
    const cards = parseVcfText(`BEGIN:VCARD
VERSION:3.0
FN:Mom [Kin]
N:;;;;
TEL:+15555550176
CATEGORIES:Family,People
END:VCARD
`);
    const draft = cardToDraft(cards[0]!);
    assert.equal(draft.firstName, "Mom");
    assert.deepEqual(draft.labels, ["Kin", "Family"]);
  });

  it("keeps the structured middle name", () => {
    const cards = parseVcfText(`BEGIN:VCARD
VERSION:3.0
FN:Ada Augusta Lovelace
N:Lovelace;Ada;Augusta;;
TEL:+15555550119
END:VCARD
`);
    const draft = cardToDraft(cards[0]!);
    assert.equal(draft.firstName, "Ada");
    assert.equal(draft.middleName, "Augusta");
    assert.equal(draft.lastName, "Lovelace");
  });

  it("parses duplicate-phone and name cards", () => {
    const drafts = parseVcfText(DUPLICATE_PHONE_VCF).map(cardToDraft);
    assert.equal(drafts.length, 3);
    assert.deepEqual(drafts[0], {
      firstName: "Ada",
      middleName: "Augusta",
      lastName: "Lovelace",
      phones: ["+15555550119"],
      labels: ["Family"],
    });
    assert.deepEqual(drafts[1]!.phones, ["+15555550119", "+15555550178"]);
    assert.deepEqual(
      [drafts[2]!.firstName, drafts[2]!.middleName, drafts[2]!.lastName],
      ["Mononym", "", ""],
    );
  });
});

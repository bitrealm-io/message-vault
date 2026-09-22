import { describe, expect, it } from "vitest";
import { readerLicenseUrl, readerSourceUrl } from "./thirdPartySoftware";

describe("readerSourceUrl", () => {
  it("points at the release tag for the Product Version of a release build", () => {
    expect(readerSourceUrl("0.9.0")).toBe(
      "https://github.com/bitrealm-io/message-vault/tree/v0.9.0/crates/helpers/imessage-reader",
    );
  });

  it("drops the commit metadata of a dev build so the link still resolves", () => {
    expect(readerSourceUrl("0.9.0+343fe0d8.dirty")).toBe(
      "https://github.com/bitrealm-io/message-vault/tree/v0.9.0/crates/helpers/imessage-reader",
    );
  });
});

describe("readerLicenseUrl", () => {
  it("points at the GPL text shipped with the reader at the same tag", () => {
    expect(readerLicenseUrl("0.9.0+343fe0d8")).toBe(
      "https://github.com/bitrealm-io/message-vault/blob/v0.9.0/crates/helpers/imessage-reader/LICENSE",
    );
  });
});

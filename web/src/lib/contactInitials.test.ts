import { describe, expect, it } from "vitest";
import { contactAvatarClass, contactInitials } from "./contactInitials";

describe("contactInitials", () => {
  it("uses first and last name letters", () => {
    expect(contactInitials({ firstName: "Ada", lastName: "Lovelace" })).toBe("AL");
  });

  it("falls back to display name words", () => {
    expect(contactInitials({ displayName: "Grace Hopper" })).toBe("GH");
  });

  it("handles Last, First display names", () => {
    expect(contactInitials({ displayName: "Hopper, Grace" })).toBe("GH");
  });

  it("returns ? when nothing usable", () => {
    expect(contactInitials({})).toBe("?");
  });
});

describe("contactAvatarClass", () => {
  it("is stable for the same person seed", () => {
    const a = contactAvatarClass({
      preferredName: "Ada",
      preferredHandle: "+15551212",
    });
    const b = contactAvatarClass({
      preferredName: "Ada",
      preferredHandle: "+1 (555) 1212",
    });
    expect(a).toBe(b);
  });

  it("differs for different people", () => {
    const a = contactAvatarClass({ preferredName: "Ada" });
    const b = contactAvatarClass({ preferredName: "Grace" });
    expect(a).not.toBe(b);
  });
});

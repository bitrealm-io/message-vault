import { describe, expect, it } from "vitest";
import {
  identityMessageCounts,
  identityOnProfile,
  identityService,
  needsIdentityStop,
  parseSourceIdentities,
} from "./backupIdentity";

const profile = { phones: ["+1 (555) 000-1111"], emails: ["Owner@Example.com"] };

describe("identityService", () => {
  it("calls anything with an @ an email and the rest a phone", () => {
    expect(identityService("owner@example.com")).toBe("email");
    expect(identityService("+15550001111")).toBe("phone");
  });
});

describe("identityOnProfile", () => {
  it("matches phones by digits despite formatting", () => {
    expect(identityOnProfile("5550001111", profile)).toBe(true);
    expect(identityOnProfile("+15559999999", profile)).toBe(false);
  });

  it("matches emails case-insensitively", () => {
    expect(identityOnProfile("owner@example.com", profile)).toBe(true);
    expect(identityOnProfile("other@example.com", profile)).toBe(false);
  });
});

describe("needsIdentityStop", () => {
  it("stops when nothing matches, including an empty profile", () => {
    expect(needsIdentityStop(["+15559999999"], profile)).toBe(true);
    expect(needsIdentityStop(["+15550001111"], { phones: [], emails: [] })).toBe(true);
  });

  it("does not stop on any overlap", () => {
    expect(needsIdentityStop(["+15559999999", "owner@example.com"], profile)).toBe(false);
  });

  it("fails open: no identities read, or no profile loaded", () => {
    expect(needsIdentityStop([], profile)).toBe(false);
    expect(needsIdentityStop(["+15559999999"], null)).toBe(false);
  });
});

describe("parseSourceIdentities", () => {
  it("keeps only an array of strings", () => {
    expect(parseSourceIdentities(["a", "b"])).toEqual(["a", "b"]);
    expect(parseSourceIdentities(["a", 5])).toBeNull();
    expect(parseSourceIdentities(null)).toBeNull();
    expect(parseSourceIdentities("a")).toBeNull();
  });
});

describe("identityMessageCounts", () => {
  it("adds up sent and received across spellings of one address", () => {
    expect(
      identityMessageCounts("+15550001111", [
        { handle: "+1 (555) 000-1111", sent: 3, received: 4 },
        { handle: "5550001111", sent: 1, received: 0 },
        { handle: "owner@example.com", sent: 9, received: 9 },
      ]),
    ).toEqual({ sent: 4, received: 4 });
  });
});

import { describe, expect, it } from "vitest";
import {
  contactBelongsToGroup,
  groupFromSlug,
  groupListQuery,
  groupSlug,
  isReservedGroupName,
  reservedGroupError,
} from "./contactGroups";
import { UNKNOWN_GROUP } from "./unknownGroup";

describe("groupSlug", () => {
  it("turns spaces and punctuation into dashes and keeps letter case", () => {
    expect(groupSlug("Work Friends")).toBe("Work-Friends");
    expect(groupSlug("  Family  ")).toBe("Family");
    expect(groupSlug("reGroup")).toBe("reGroup");
  });
});

describe("groupFromSlug", () => {
  it("prefers an exact slug match, then ignores case", () => {
    expect(groupFromSlug("Work-Friends", ["Work Friends", "Family"])).toBe("Work Friends");
    expect(groupFromSlug("family", ["Family"])).toBe("Family");
    expect(groupFromSlug("missing", ["Family"])).toBeNull();
  });
});

describe("reserved groups", () => {
  it("blocks Contacts, Trash, and No group", () => {
    expect(isReservedGroupName("Contacts")).toBe(true);
    expect(isReservedGroupName("no group")).toBe(true);
    expect(reservedGroupError("Trash")).toBe("Trash is a reserved group");
    expect(isReservedGroupName("Family")).toBe(false);
  });
});

describe("contactBelongsToGroup", () => {
  const named = (groups: string[] | undefined) => ({ groups, unknown: false });
  const unknown = (groups: string[] | undefined) => ({ groups, unknown: true });

  it("keeps every contact when no group page is active", () => {
    expect(contactBelongsToGroup(named(["Family"]), null)).toBe(true);
    expect(contactBelongsToGroup(named([]), null)).toBe(true);
    expect(contactBelongsToGroup(unknown([]), null)).toBe(true);
  });

  it("matches group names without regard to letter case", () => {
    expect(contactBelongsToGroup(named(["Family"]), "family")).toBe(true);
    expect(contactBelongsToGroup(named(["Work"]), "Family")).toBe(false);
  });

  it("treats unknown as the contacts the server marked Unknown", () => {
    expect(contactBelongsToGroup(unknown([]), UNKNOWN_GROUP)).toBe(true);
    expect(contactBelongsToGroup(unknown(undefined), UNKNOWN_GROUP)).toBe(true);
    expect(contactBelongsToGroup(unknown(["Family"]), UNKNOWN_GROUP)).toBe(true);
    expect(contactBelongsToGroup(named([]), UNKNOWN_GROUP)).toBe(false);
    expect(contactBelongsToGroup(named(["Family"]), UNKNOWN_GROUP)).toBe(false);
  });

  it("treats none as contacts with no stored group that are not Unknown", () => {
    expect(contactBelongsToGroup(named([]), "none")).toBe(true);
    expect(contactBelongsToGroup(named(undefined), "none")).toBe(true);
    expect(contactBelongsToGroup(named(["Family"]), "none")).toBe(false);
    // An Unknown contact with no stored group is in Unknown, so it is not in
    // No group. The server leaves it out of `group:none` too.
    expect(contactBelongsToGroup(unknown([]), "none")).toBe(false);
    expect(contactBelongsToGroup(unknown(undefined), "none")).toBe(false);
  });
});

describe("groupListQuery", () => {
  it("quotes names that contain spaces and keeps typed search", () => {
    expect(groupListQuery("Family", "")).toBe("group:Family");
    expect(groupListQuery("Work Friends", "ada")).toBe('group:"Work Friends" ada');
    expect(groupListQuery("none", "bob")).toBe("group:none bob");
    expect(groupListQuery(null, "ada")).toBe("ada");
  });

  it("quotes a name with parentheses, since the language reads them as grouping", () => {
    expect(groupListQuery("Family (close)", "")).toBe('group:"Family (close)"');
  });
});

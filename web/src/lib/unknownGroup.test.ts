import { describe, expect, it } from "vitest";
import { contactBelongsToGroup, groupListQuery } from "./contactGroups";
import { UNKNOWN_GROUP } from "./unknownGroup";

describe("the Unknown contact group", () => {
  it("asks the server for it by name", () => {
    expect(groupListQuery(UNKNOWN_GROUP, "")).toBe("group:unknown");
  });

  it("keeps a typed search alongside it", () => {
    expect(groupListQuery(UNKNOWN_GROUP, "messages:>0")).toBe("group:unknown messages:>0");
  });

  it("does not re-filter rows the server already chose", () => {
    // Unknown membership is computed from contact state, so it never appears
    // among a contact's stored group names. Re-checking here would throw away
    // every row the server returned.
    expect(contactBelongsToGroup([], UNKNOWN_GROUP)).toBe(true);
    expect(contactBelongsToGroup(undefined, UNKNOWN_GROUP)).toBe(true);
    expect(contactBelongsToGroup(["Family"], UNKNOWN_GROUP)).toBe(true);
  });

  it("still filters the groups a person made", () => {
    expect(contactBelongsToGroup(["Family"], "Family")).toBe(true);
    expect(contactBelongsToGroup(["Work"], "Family")).toBe(false);
    expect(contactBelongsToGroup([], "none")).toBe(true);
    expect(contactBelongsToGroup(["Family"], "none")).toBe(false);
  });
});

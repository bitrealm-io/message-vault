import { describe, expect, it } from "vitest";
import { groupListQuery } from "./contactGroups";
import { UNKNOWN_GROUP } from "./unknownGroup";

describe("the Unknown contact group", () => {
  it("asks the server for it by name", () => {
    expect(groupListQuery(UNKNOWN_GROUP, "")).toBe("group:unknown");
  });

  it("keeps a typed search alongside it", () => {
    expect(groupListQuery(UNKNOWN_GROUP, "messages:>0")).toBe("group:unknown messages:>0");
  });
});

import { describe, expect, it } from "vitest";
import {
  compareContacts,
  compareContactsByName,
  contactSortLetter,
  groupByLetter,
  splitContactName,
  withSortField,
} from "./contactSort.ts";

describe("splitContactName", () => {
  it("uses the last word as the last name", () => {
    expect(splitContactName("Alice Green")).toEqual({
      first: "Alice",
      last: "Green",
    });
    expect(splitContactName("Mary Ann Lee")).toEqual({
      first: "Mary",
      last: "Lee",
    });
  });

  it("reads Last, First", () => {
    expect(splitContactName("Lee, Alice")).toEqual({
      first: "Alice",
      last: "Lee",
    });
  });

  it("repeats a single word for both fields", () => {
    expect(splitContactName("Aaliyah")).toEqual({
      first: "Aaliyah",
      last: "Aaliyah",
    });
  });
});

describe("contactSortLetter", () => {
  it("takes the first letter of the active field", () => {
    expect(contactSortLetter("Alice Green", "first")).toBe("A");
    expect(contactSortLetter("Alice Green", "last")).toBe("G");
  });

  it("uses # when the name does not start with A–Z", () => {
    expect(contactSortLetter("+15555550123", "last")).toBe("#");
    expect(contactSortLetter("", "first")).toBe("#");
  });
});

describe("groupByLetter", () => {
  it("keeps sort order and starts a group when the letter changes", () => {
    const names = ["Amy Adams", "Zoe Adams", "Bob Lee"];
    const groups = groupByLetter(names, (n) => contactSortLetter(n, "last"));
    expect(groups.map(([letter, items]) => [letter, [...items]])).toEqual([
      ["A", ["Amy Adams", "Zoe Adams"]],
      ["L", ["Bob Lee"]],
    ]);
  });
});

describe("compareContactsByName", () => {
  it("sorts by last name then first name", () => {
    const names = ["Zoe Adams", "Amy Adams", "Bob Lee"];
    const sorted = [...names].sort((a, b) => compareContactsByName(a, b, "last", "asc"));
    expect(sorted).toEqual(["Amy Adams", "Zoe Adams", "Bob Lee"]);
  });

  it("sorts by first name then last name", () => {
    const names = ["Zoe Adams", "Amy Adams", "Bob Lee"];
    const sorted = [...names].sort((a, b) => compareContactsByName(a, b, "first", "asc"));
    expect(sorted).toEqual(["Amy Adams", "Bob Lee", "Zoe Adams"]);
  });
});

describe("compareContacts by last heard", () => {
  const rows = [
    { name: "Silent", handles: [], last_heard_at: null },
    { name: "Older", handles: [], last_heard_at: "2024-03-01T00:00:00Z" },
    { name: "Recent", handles: [], last_heard_at: "2024-06-01T00:00:00Z" },
    { name: "Quiet", handles: [] },
  ];
  const names = (order: "asc" | "desc") =>
    [...rows]
      .sort((a, b) => compareContacts(a, b, { sort: "lastHeard", order }))
      .map((r) => r.name);

  it("puts the contact heard from most recently first when descending", () => {
    expect(names("desc")).toEqual(["Recent", "Older", "Quiet", "Silent"]);
  });

  it("keeps contacts never heard from last in either direction, A to Z among themselves", () => {
    expect(names("asc")).toEqual(["Older", "Recent", "Quiet", "Silent"]);
  });
});

describe("withSortField", () => {
  it("starts last heard newest first and a name A to Z", () => {
    expect(withSortField({ sort: "last", order: "asc" }, "lastHeard")).toEqual({
      sort: "lastHeard",
      order: "desc",
    });
    expect(withSortField({ sort: "lastHeard", order: "desc" }, "first")).toEqual({
      sort: "first",
      order: "asc",
    });
  });

  it("leaves the order alone when the field does not change", () => {
    const state = { sort: "last", order: "desc" } as const;
    expect(withSortField(state, "last")).toBe(state);
  });
});

import { describe, expect, it } from "vitest";
import { conversationName, isGroupType } from "./conversationName";

const alice = { name: "Alice" };
const bob = { name: "Bob" };

describe("conversationName", () => {
  it("uses the title when there is one", () => {
    expect(conversationName({ title: " Book club ", isGroup: true, participants: [alice] })).toBe(
      "Book club",
    );
  });

  it("names a one-to-one conversation after the other person", () => {
    expect(conversationName({ title: null, isGroup: false, participants: [alice, bob] })).toBe(
      "Alice",
    );
  });

  it("names an untitled group after every participant", () => {
    expect(conversationName({ title: "", isGroup: true, participants: [alice, bob] })).toBe(
      "Alice, Bob",
    );
  });

  it("says (unknown) when there is nobody to name", () => {
    expect(conversationName({ title: null, isGroup: false, participants: [] })).toBe("(unknown)");
    expect(conversationName({ title: null, isGroup: true, participants: [] })).toBe("(unknown)");
  });
});

describe("isGroupType", () => {
  it("ignores case, as the server does", () => {
    expect(isGroupType("group")).toBe(true);
    expect(isGroupType("Group")).toBe(true);
    expect(isGroupType("individual")).toBe(false);
  });
});

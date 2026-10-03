import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setAccountId } from "./api.ts";
import { clearRecentSearches, loadRecentSearches, pushRecentSearch } from "./recentSearches.ts";
import { accountKey } from "./storage.ts";

const mem = new Map<string, string>();

beforeEach(() => {
  mem.clear();
  const store: Storage = {
    getItem: (k) => mem.get(k) ?? null,
    setItem: (k, v) => {
      mem.set(k, String(v));
    },
    removeItem: (k) => {
      mem.delete(k);
    },
    clear: () => mem.clear(),
    key: () => null,
    length: 0,
  };
  // The node environment has no window. lib/storage.ts reads window.localStorage.
  vi.stubGlobal("localStorage", store);
  vi.stubGlobal("window", { localStorage: store });
  setAccountId(1);
});

afterEach(() => {
  setAccountId(null);
  vi.unstubAllGlobals();
});

describe("recentSearches", () => {
  it("returns empty for missing or corrupt JSON", () => {
    expect(loadRecentSearches("contact")).toEqual([]);
    mem.set(accountKey(1, "contact-recent-searches"), "{not-json");
    expect(loadRecentSearches("contact")).toEqual([]);
  });

  it("pushes newest first, dedupes, and caps at 10", () => {
    for (let i = 0; i < 12; i++) pushRecentSearch("contact", `q${i}`);
    const list = loadRecentSearches("contact");
    expect(list).toHaveLength(10);
    expect(list[0]).toBe("q11");
    expect(list[9]).toBe("q2");
    pushRecentSearch("contact", "q5");
    expect(loadRecentSearches("contact")[0]).toBe("q5");
    expect(loadRecentSearches("contact").filter((q) => q === "q5")).toHaveLength(1);
  });

  it("ignores empty and whitespace-only pushes", () => {
    pushRecentSearch("contact", "  ");
    expect(loadRecentSearches("contact")).toEqual([]);
  });

  it("clear removes the key", () => {
    pushRecentSearch("contact", "alice");
    clearRecentSearches("contact");
    expect(loadRecentSearches("contact")).toEqual([]);
  });

  it("keeps each bar's history separate", () => {
    pushRecentSearch("contact", "alice");
    pushRecentSearch("message", "identity:+15555550100");
    pushRecentSearch("trash", "birthday");

    expect(loadRecentSearches("contact")).toEqual(["alice"]);
    expect(loadRecentSearches("message")).toEqual(["identity:+15555550100"]);
    expect(loadRecentSearches("trash")).toEqual(["birthday"]);

    clearRecentSearches("message");
    expect(loadRecentSearches("contact")).toEqual(["alice"]);
    expect(loadRecentSearches("message")).toEqual([]);
    expect(loadRecentSearches("trash")).toEqual(["birthday"]);
  });
});

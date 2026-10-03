import { describe, expect, it } from "vitest";
import { messagesSearch, openedAt, pickedMessageSort, resultsView } from "./resultsView";

const params = (s: string) => new URLSearchParams(s);

describe("resultsView", () => {
  it("is Messages only when the address says so", () => {
    expect(resultsView(params("view=messages"))).toBe("messages");
    expect(resultsView(params(""))).toBe("conversations");
    expect(resultsView(params("view=other"))).toBe("conversations");
  });
});

describe("pickedMessageSort", () => {
  it("reads the route's spelling, and nothing else", () => {
    expect(pickedMessageSort(params("msort=relevance"))).toEqual({
      sort: "relevance",
      order: "desc",
    });
    expect(pickedMessageSort(params("msort=date"))).toEqual({ sort: "date", order: "asc" });
    expect(pickedMessageSort(params("msort=-date"))).toEqual({ sort: "date", order: "desc" });
    expect(pickedMessageSort(params("msort=colour"))).toBeNull();
  });
});

describe("openedAt", () => {
  it("is a positive message id, or null", () => {
    expect(openedAt(params("at=42"))).toBe(42);
    expect(openedAt(params("at=0"))).toBeNull();
    expect(openedAt(params("at=4x"))).toBeNull();
    expect(openedAt(params(""))).toBeNull();
  });
});

describe("messagesSearch", () => {
  it("carries the search and the Messages list's parameters, and drops the filter", () => {
    expect(
      messagesSearch(params("q=photo&f=with%3A%2342&view=messages&msort=date&at=7"), {
        at: "9",
      }),
    ).toBe("?q=photo&view=messages&msort=date&at=9");
  });

  it("drops a parameter set to empty, and is empty when nothing is left", () => {
    expect(messagesSearch(params("q=photo&view=messages"), { view: "", q: "" })).toBe("");
  });
});

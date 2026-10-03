import { describe, expect, it } from "vitest";
import { effectiveMessageSort, messageSortParam } from "./messageSearchSort";

describe("effectiveMessageSort", () => {
  it("is Relevance by default when the query has a free-text word, and Date newest first otherwise", () => {
    expect(effectiveMessageSort(null, true)).toEqual({ sort: "relevance", order: "desc" });
    expect(effectiveMessageSort(null, false)).toEqual({ sort: "date", order: "desc" });
  });

  it("keeps the person's pick, except Relevance with nothing to rank by", () => {
    expect(effectiveMessageSort({ sort: "date", order: "asc" }, true)).toEqual({
      sort: "date",
      order: "asc",
    });
    expect(effectiveMessageSort({ sort: "relevance", order: "desc" }, false)).toEqual({
      sort: "date",
      order: "desc",
    });
  });
});

describe("messageSortParam", () => {
  it("spells the sort as the route takes it", () => {
    expect(messageSortParam({ sort: "relevance", order: "desc" })).toBe("relevance");
    expect(messageSortParam({ sort: "relevance", order: "asc" })).toBe("relevance");
    expect(messageSortParam({ sort: "date", order: "asc" })).toBe("date");
    expect(messageSortParam({ sort: "date", order: "desc" })).toBe("-date");
  });
});

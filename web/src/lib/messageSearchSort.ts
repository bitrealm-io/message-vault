import type { SortOrder } from "../components/SortMenu";

/** What the Messages list can be sorted by. */
export type MessageSearchSortKey = "relevance" | "date";

/** One choice in the Messages list's sort menu. Relevance has no order: best match first. */
export type MessageSearchSort = { sort: MessageSearchSortKey; order: SortOrder };

/**
 * The order the Messages list shows: the one the person picked, unless it is
 * Relevance and the query has no free-text word to rank by. With no pick,
 * Relevance when there is a word to rank by, and otherwise Date, newest first.
 */
export function effectiveMessageSort(
  picked: MessageSearchSort | null,
  rankable: boolean,
): MessageSearchSort {
  if (picked && (picked.sort === "date" || rankable)) return picked;
  return rankable ? { sort: "relevance", order: "desc" } : { sort: "date", order: "desc" };
}

/** The `sort` parameter `GET /v1/messages` takes for `s`. */
export function messageSortParam(s: MessageSearchSort): "relevance" | "date" | "-date" {
  if (s.sort === "relevance") return "relevance";
  return s.order === "asc" ? "date" : "-date";
}

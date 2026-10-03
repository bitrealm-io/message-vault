import type { SortOrder } from "../components/SortMenu";
import type { MessagesListParams } from "./serverApi";

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

/** A `sort` value `GET /v1/messages` takes. */
export type MessageSortParam = NonNullable<MessagesListParams["sort"]>;

/**
 * The `sort` value for each choice, read both ways: Relevance has one order,
 * and Date one value per order.
 */
const SORT_PARAMS: Readonly<Record<"relevance" | SortOrder, MessageSortParam>> = {
  relevance: "relevance",
  asc: "date",
  desc: "-date",
};

/** The `sort` parameter `GET /v1/messages` takes for `s`. */
export function messageSortParam(s: MessageSearchSort): MessageSortParam {
  return SORT_PARAMS[s.sort === "relevance" ? "relevance" : s.order];
}

/** The choice a `sort` value stands for, or null for a value it is not. */
export function messageSortFromParam(param: string | null): MessageSearchSort | null {
  if (param === SORT_PARAMS.relevance) return { sort: "relevance", order: "desc" };
  if (param === SORT_PARAMS.asc) return { sort: "date", order: "asc" };
  if (param === SORT_PARAMS.desc) return { sort: "date", order: "desc" };
  return null;
}

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

/** Each `sort` value and the choice it stands for, read both ways. */
const SORT_PARAMS: ReadonlyArray<readonly [MessageSortParam, MessageSearchSort]> = [
  ["relevance", { sort: "relevance", order: "desc" }],
  ["date", { sort: "date", order: "asc" }],
  ["-date", { sort: "date", order: "desc" }],
];

/** The `sort` parameter `GET /v1/messages` takes for `s`. Relevance has one order. */
export function messageSortParam(s: MessageSearchSort): MessageSortParam {
  const found = SORT_PARAMS.find(
    ([, choice]) => choice.sort === s.sort && (s.sort === "relevance" || choice.order === s.order),
  );
  return found ? found[0] : "-date";
}

/** The choice a `sort` value stands for, or null for a value it is not. */
export function messageSortFromParam(param: string | null): MessageSearchSort | null {
  return SORT_PARAMS.find(([value]) => value === param)?.[1] ?? null;
}

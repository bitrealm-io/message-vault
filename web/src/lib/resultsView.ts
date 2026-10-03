import type { MessageSearchSort } from "./messageSearchSort";

/**
 * What the Messages screen's results list: the conversations a search
 * matches, or the messages it matches across every conversation (#313). It
 * rides in the address as `view=messages`, so it survives opening a
 * conversation and a reload, and Conversations is the address without it.
 */
export type ResultsView = "conversations" | "messages";

/** The address parameters the Messages list keeps. */
export const VIEW_PARAM = "view";
/** The sort the person picked in the Messages list, as the route spells it. */
export const MESSAGE_SORT_PARAM = "msort";
/** The message a search result opened its conversation at. */
export const AT_PARAM = "at";

/** Which results `params` asks for. */
export function resultsView(params: URLSearchParams): ResultsView {
  return params.get(VIEW_PARAM) === "messages" ? "messages" : "conversations";
}

/** The sort the person picked in the Messages list, or null for the default. */
export function pickedMessageSort(params: URLSearchParams): MessageSearchSort | null {
  switch (params.get(MESSAGE_SORT_PARAM)) {
    case "relevance":
      return { sort: "relevance", order: "desc" };
    case "date":
      return { sort: "date", order: "asc" };
    case "-date":
      return { sort: "date", order: "desc" };
    default:
      return null;
  }
}

/** The message a search result opened at: a positive integer, or null. */
export function openedAt(params: URLSearchParams): number | null {
  const raw = params.get(AT_PARAM);
  if (raw === null || !/^\d+$/.test(raw)) return null;
  const n = Number(raw);
  return Number.isSafeInteger(n) && n > 0 ? n : null;
}

/**
 * The address search for a page of the Messages screen: `q` and the Messages
 * list's own parameters from `current`, with `overrides` on top. An empty
 * value drops the parameter. `f` is not carried: a typed search replaces it.
 */
export function messagesSearch(
  current: URLSearchParams,
  overrides: Record<string, string>,
): string {
  const next = new URLSearchParams();
  for (const key of ["q", VIEW_PARAM, MESSAGE_SORT_PARAM, AT_PARAM]) {
    const value = key in overrides ? overrides[key] : current.get(key);
    if (value) next.set(key, value);
  }
  const s = next.toString();
  return s ? `?${s}` : "";
}

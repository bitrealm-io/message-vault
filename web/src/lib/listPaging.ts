/**
 * Page sizes and the labels a long list shows while it fills.
 *
 * These outlived `usePagedList`, whose fetching, caching, and paging are now
 * TanStack Query's job. What is left is arithmetic and wording, with no state
 * of its own.
 */

/** Rows in the first page of a searched list. */
export const PAGE_SIZE_FIRST = 40;
/** Rows in each page loaded as the person scrolls. */
export const PAGE_SIZE_FILL = 100;
/**
 * The largest `offset` a browse list accepts (`docs/architecture/http-api.md`,
 * "Lists"). A list that pages past it ends there. It mirrors the server's
 * `MAX_LIST_OFFSET` in `crates/server/server/src/paging.rs`, which the
 * OpenAPI reference states only in prose, so change the two together.
 */
export const MAX_LIST_OFFSET = 50_000;
/** Contacts catalog first page — large enough for typical accounts in one request. */
export const PAGE_SIZE_CONTACTS_FIRST = 500;

/** Status suffix appended to a visible-range label. */
export function listActivitySuffix(refreshing: boolean, filling: boolean): string {
  if (refreshing) return " · updating…";
  if (filling) return " · loading more…";
  return "";
}

/**
 * Build a "1–20 of 100" label for the rows currently on screen.
 * Uses 1-based start and end indexes. Shows "… of N" until the list reports a window.
 */
export function formatVisibleRange(
  visibleStart: number,
  visibleEnd: number,
  total: number,
  itemCount: number,
): string {
  if (total === 0 && itemCount === 0) return "0 of 0";
  if (itemCount === 0) return `0 of ${total}`;
  if (visibleStart < 1 || visibleEnd < 1) return `… of ${total}`;
  const start = Math.min(visibleStart, itemCount);
  const end = Math.max(start, Math.min(visibleEnd, itemCount));
  return `${start}–${end} of ${total}`;
}

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
/** Contacts catalog first page — large enough for typical accounts in one request. */
export const PAGE_SIZE_CONTACTS_FIRST = 500;
/** The largest page any list route answers (`MAX_LIST_LIMIT` in the server's `paging.rs`). */
export const PAGE_SIZE_MAX = 500;

/** How close to the end of a list, in pixels, scrolling asks for the next page. */
export const NEAR_END_PX = 48;

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

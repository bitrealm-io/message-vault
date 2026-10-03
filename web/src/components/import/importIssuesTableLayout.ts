/** The column header row above the scrolled rows. */
export const HEADER_ROW_HEIGHT = 36;
/** Collapsed row: file + step + two lines of error text. */
export const COLLAPSED_ROW_HEIGHT = 56;
export const MAX_VISIBLE_ROWS = 14;
export const MAX_VISIBLE_FILENAMES = 6;
export const FILENAME_ROW_PX = 20;

function estimateReasonHeight(reason: string): number {
  // Rough wrap estimate for the error column (~42 chars/line at this font size).
  const lines = Math.max(2, Math.ceil(reason.length / 42));
  return Math.min(220, 20 + lines * 18);
}

export function estimateExpandedHeight(reason: string, fileCount: number): number {
  const reasonHeight = estimateReasonHeight(reason);
  if (fileCount <= 1) return reasonHeight;
  const visibleFiles = Math.min(fileCount, MAX_VISIBLE_FILENAMES);
  return reasonHeight + 8 + visibleFiles * FILENAME_ROW_PX;
}

/** Scrollport height: collapsed rows, plus extra for the expanded row, capped at 14 rows. */
export function tableViewportHeight(
  groupCount: number,
  expanded: { readonly reason: string; readonly fileCount: number } | null,
): number {
  const collapsedViewport = Math.min(groupCount, MAX_VISIBLE_ROWS) * COLLAPSED_ROW_HEIGHT;
  if (expanded == null) {
    return collapsedViewport;
  }
  const extra = Math.max(
    0,
    estimateExpandedHeight(expanded.reason, expanded.fileCount) - COLLAPSED_ROW_HEIGHT,
  );
  return Math.min(collapsedViewport + extra, MAX_VISIBLE_ROWS * COLLAPSED_ROW_HEIGHT);
}

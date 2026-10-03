import { listActivitySuffix } from "../lib/listPaging";
import { Z_RANGE_PILL } from "../lib/zLayers";

/*
  Room under the last row so the floating range pill does not cover a row:
  3.5rem (56px), as a list box's bottom padding or as a spacer after the rows.
  `styleTokens.test.ts` checks that the two are the same size.
*/
export const RANGE_PILL_SCROLL_PAD_CLASS = "pb-14";
/** The same room as a height, for the spacer after a list's rows. */
export const RANGE_PILL_SPACER_CLASS = "h-14";

/** The spacer after a list's rows, `RANGE_PILL_SPACER_CLASS` high. */
export function RangePillSpacer() {
  return <div aria-hidden className={`shrink-0 ${RANGE_PILL_SPACER_CLASS}`} />;
}

/** Viewport pixels the pill covers (`bottom-3` + pill). Range math ignores this band. */
export const RANGE_PILL_OVERLAY_INSET = 40;

/**
 * Floating "1–20 of 100" marker pinned to the bottom of a list panel. It sits
 * over the last rows rather than taking a row of its own, so the list keeps the
 * full height of the panel.
 */
export default function ListRangePill({
  rangeLabel,
  refreshing = false,
  filling = false,
  testId = "list-range-pill",
}: {
  rangeLabel: string;
  refreshing?: boolean;
  filling?: boolean;
  testId?: string;
}) {
  return (
    <div
      className={`pointer-events-none absolute inset-x-0 bottom-3 flex justify-center ${Z_RANGE_PILL}`}
    >
      <span
        data-testid={testId}
        className="rounded-full border border-border bg-elevated px-2.5 py-1 text-[0.688rem] tabular-nums text-text shadow-pill"
      >
        {rangeLabel}
        {listActivitySuffix(refreshing, filling)}
      </span>
    </div>
  );
}

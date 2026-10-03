import type { ReactNode } from "react";
import { listActivitySuffix } from "../lib/listPaging";
import Checkbox from "./Checkbox";

/** Same height on the sidebar spacer, list toolbar, and right-pane toolbar. */
export const LIST_TOOLBAR_CLASS =
  "flex h-9 shrink-0 items-center gap-2.5 border-b border-border px-3";

/** The Select all box at the start of a list toolbar. */
export type SelectAllBox = {
  checked?: boolean;
  indeterminate?: boolean;
  onChange: (checked: boolean) => void;
  label?: string;
  /** The box waits, as while Select all loads every page. */
  disabled?: boolean;
  /** Why Select all ticked nothing, shown under the toolbar. */
  error?: string | null;
};

/** Shared list toolbar chrome for conversation and contact lists. */
export default function ListRangeHeader({
  rangeLabel,
  refreshing = false,
  filling = false,
  actions,
  selectAll,
}: {
  /** When omitted, the center stays empty so actions stay right-aligned. */
  rangeLabel?: string;
  refreshing?: boolean;
  filling?: boolean;
  /** Right side of the range row (sort, groups, tags). */
  actions?: ReactNode;
  /** The Select all box; a list without one leaves it out. */
  selectAll?: SelectAllBox;
}) {
  const activitySuffix = listActivitySuffix(refreshing, filling);

  return (
    <>
      <div className={LIST_TOOLBAR_CLASS}>
        {selectAll ? (
          <span className="flex h-7 w-7 shrink-0 items-center justify-center">
            <Checkbox
              checked={selectAll.checked ?? false}
              indeterminate={selectAll.indeterminate ?? false}
              disabled={selectAll.disabled ?? false}
              aria-label={selectAll.label ?? "Select all"}
              onChange={(on) => selectAll.onChange(on)}
            />
          </span>
        ) : null}
        <span className="min-w-0 flex-1 truncate text-[0.688rem] text-muted">
          {rangeLabel != null ? (
            <>
              {rangeLabel}
              {activitySuffix}
            </>
          ) : null}
        </span>
        {actions ? <div className="shrink-0">{actions}</div> : null}
      </div>
      {selectAll?.error ? (
        <p
          role="alert"
          className="shrink-0 border-b border-border px-3 py-1.5 text-[0.75rem] text-danger"
        >
          {selectAll.error}
        </p>
      ) : null}
    </>
  );
}

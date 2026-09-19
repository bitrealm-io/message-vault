import type { ReactNode } from "react";
import { listActivitySuffix } from "../lib/listPaging";
import Checkbox from "./Checkbox";

/** Same height on the sidebar spacer, list toolbar, and right-pane toolbar. */
export const LIST_TOOLBAR_CLASS =
  "flex h-9 shrink-0 items-center gap-2.5 border-b border-border px-3";

/** Shared list toolbar chrome for conversation and contact lists. */
export default function ListRangeHeader({
  rangeLabel,
  refreshing = false,
  filling = false,
  actions,
  selectAllChecked = false,
  selectAllIndeterminate = false,
  onSelectAllChange,
  selectAllLabel = "Select all",
  selectAllDisabled = false,
}: {
  /** When omitted, the center stays empty so actions stay right-aligned. */
  rangeLabel?: string;
  refreshing?: boolean;
  filling?: boolean;
  /** Right side of the range row (sort, groups, tags). */
  actions?: ReactNode;
  selectAllChecked?: boolean;
  selectAllIndeterminate?: boolean;
  onSelectAllChange?: (checked: boolean) => void;
  selectAllLabel?: string;
  selectAllDisabled?: boolean;
}) {
  const activitySuffix = listActivitySuffix(refreshing, filling);

  return (
    <div className={LIST_TOOLBAR_CLASS}>
      {onSelectAllChange ? (
        <span className="flex h-7 w-7 shrink-0 items-center justify-center">
          <Checkbox
            checked={selectAllChecked}
            indeterminate={selectAllIndeterminate}
            disabled={selectAllDisabled}
            aria-label={selectAllLabel}
            onChange={(on) => onSelectAllChange(on)}
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
  );
}

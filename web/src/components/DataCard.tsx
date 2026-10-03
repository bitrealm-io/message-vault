import type { ReactNode } from "react";

/** Header row band — distinct from the card body. */
export const dataCardHeaderRowClass = "border-b border-border bg-elevated";

/**
 * Sortable / static header cells on the header band. The focus ring is drawn
 * inside the cell: the card's scroll region clips a ring drawn outside it.
 */
export const dataCardHeaderCellClass =
  "px-2 py-2 text-center text-[0.688rem] font-semibold uppercase tracking-[0.04em] text-text outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset cursor-pointer hover:text-accent data-hovered:text-accent";

/** Primary body cells. */
export const dataCardBodyCellClass =
  "px-3 py-2.5 align-middle text-center text-[0.813rem] leading-snug text-text";

/**
 * Bordered data card shell: panel surface, optional title/toolbar, scroll region.
 * Callers supply table markup (e.g. React Aria Table) as children.
 */
export default function DataCard({
  children,
  title,
  toolbar,
  intro,
  className = "",
  maxWidthClass = "max-w-4xl",
  bodyClassName = "overflow-x-auto",
}: {
  children: ReactNode;
  title?: ReactNode;
  toolbar?: ReactNode;
  /** Content between the title row and the table (groups, filters, etc.). */
  intro?: ReactNode;
  className?: string;
  maxWidthClass?: string;
  bodyClassName?: string;
}) {
  const hasHeader = title != null || toolbar != null;
  return (
    <div
      className={`w-full ${maxWidthClass} rounded-lg border border-border bg-panel p-4 ${className}`.trim()}
    >
      {hasHeader ? (
        <div className="mb-3 flex items-start justify-between gap-3">
          {title != null ? (
            <div className="min-w-0 flex-1 text-[0.938rem] font-semibold text-text">{title}</div>
          ) : (
            <span />
          )}
          {toolbar ? <div className="shrink-0">{toolbar}</div> : null}
        </div>
      ) : null}
      {intro != null ? <div className="mb-4">{intro}</div> : null}
      <div className={bodyClassName}>{children}</div>
    </div>
  );
}

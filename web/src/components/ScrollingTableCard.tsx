import type { ReactNode } from "react";

interface ScrollingTableCardProps {
  /** Classes for the scroller: spacing around the card. */
  className?: string;
  /** Classes for the card: its corner radius and background. */
  cardClassName?: string;
  children: ReactNode;
}

/**
 * A bordered card around a table that scrolls sideways when the table is
 * wider than the space it has. The scroller is a plain box and the rounded
 * card sits inside it, growing to the table's width. A scroller with rounded
 * corners of its own makes the desktop app draw the table's text thinner
 * once the table overflows.
 */
export default function ScrollingTableCard({
  className = "",
  cardClassName = "",
  children,
}: ScrollingTableCardProps) {
  return (
    <div className={`overflow-x-auto ${className}`}>
      <div className={`w-max min-w-full border border-border ${cardClassName}`}>{children}</div>
    </div>
  );
}

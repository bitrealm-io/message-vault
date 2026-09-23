/**
 * The look Owner Home's tables share: User Accounts and the Dashboard's
 * Messages by account. Written once so the two cannot drift apart.
 */

/** A column heading: bold, in the text color, so it stands apart from the rows. */
export const thClass = "px-3 py-2 text-left text-[0.75rem] font-bold text-text";

/** The line between one column heading and the next. */
export const thSeparator = "border-l border-border";

/**
 * Every other row is a shade lighter. The shade is on the cells, and the last
 * row's end cells are rounded, so it follows the card's bottom corners; the
 * card cannot clip it, because a rounded clipping box thins the table's text
 * in the desktop app.
 */
export const rowStripe =
  "even:[&>td]:bg-hover/50 last:[&>td:first-child]:rounded-bl-[0.6875rem] last:[&>td:last-child]:rounded-br-[0.6875rem]";

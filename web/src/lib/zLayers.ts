/**
 * The overlay stacking ladder, as Tailwind classes.
 *
 * These were scattered as bare `z-[70]` / `z-[80]` / `z-[250]` literals whose
 * relationships you had to reconstruct by grepping. Each rung is named for what
 * sits on it and says what it must clear, so a new overlay picks a rung instead
 * of guessing a number. Keep in sync with the ladder in STYLE_GUIDE.md.
 */

/**
 * Lifts an element one step above its siblings, inside whatever stacking
 * context they share: a list row's lead cell over the row's stretched select
 * button, a table's column grip over the header cell beside it.
 */
export const Z_LIFT = "z-[1]";

/** The floating range pill ("1–20 of 100") over the rows of its list. */
export const Z_RANGE_PILL = "z-10";

/** The app header, so the menus it opens paint over the panels below it. */
export const Z_APP_HEADER = "z-20";

/**
 * Column resize handles: above content, below the drawers and every panel
 * opened over them, so an open drawer covers the handles under it.
 */
export const Z_RESIZE_HANDLE = "z-30";

/**
 * The overlay contact drawer, opened from a conversation. It is not modal and
 * stays open while other dialogs open, so it sits above the resize handles and
 * below every scrim: a dialog opened over it dims it and takes its clicks.
 */
export const Z_CONTACT_DRAWER = "z-[35]";

/** Backdrop behind a modal drawer. */
export const Z_DRAWER_SCRIM = "z-40";

/** A modal drawer panel (the Sources panel), above its scrim. */
export const Z_DRAWER = "z-50";

/** Advanced-search panel and similar inline overlays; must clear the resize handle. */
export const Z_INLINE_PANEL = "z-[70]";

/** The little rotated square that points from an inline panel at its trigger. */
export const Z_INLINE_PANEL_TAIL = "z-[71]";

/** Select and ComboBox popovers, and menus. */
export const Z_POPOVER = "z-[100]";

/** Modal dialogs and the attachment lightbox. */
export const Z_MODAL = "z-[200]";

/**
 * A select popover opened from inside a modal, which has to clear the modal
 * itself. Needs `!` because it overrides `Z_POPOVER` baked into the Select.
 */
export const Z_POPOVER_IN_MODAL = "!z-[250]";

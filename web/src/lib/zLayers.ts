/**
 * The overlay stacking ladder, as Tailwind classes.
 *
 * These were scattered as bare `z-[70]` / `z-[80]` / `z-[250]` literals whose
 * relationships you had to reconstruct by grepping. Each rung is named for what
 * sits on it and says what it must clear, so a new overlay picks a rung instead
 * of guessing a number. Keep in sync with the ladder in STYLE_GUIDE.md.
 */

/** Backdrop behind the right-edge drawer. */
export const Z_DRAWER_SCRIM = "z-40";

/** The drawer panel itself, above its scrim. */
export const Z_DRAWER = "z-50";

/** Column resize handles — above content, below any panel opened over them. */
export const Z_RESIZE_HANDLE = "z-[60]";

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

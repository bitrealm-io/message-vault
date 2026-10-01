/**
 * CSS selectors for menus and calendars that render outside the search panel.
 *
 * Only marked overlays and modal backdrops match. Contact and conversation
 * lists also use listbox roles; treating those as overlays stopped outside
 * clicks from closing the search panel.
 */
const PORTALED_OVERLAY_SELECTOR = ["[data-mc-overlay]", "[data-testid='underlay']"].join(", ");

/**
 * True when an outside click should leave the parent panel open.
 *
 * Call this from a capture-phase listener so the menu backdrop is still in the
 * click path. Clicks on the panel itself, or on a menu that rendered outside
 * it, keep the panel open. The menu library closes the menu on its own.
 */
export function shouldIgnoreOutsideDismiss(
  event: MouseEvent | PointerEvent,
  root: Node | null,
): boolean {
  if (!root) return true;

  const path = event.composedPath();
  for (const node of path) {
    if (!(node instanceof Element)) continue;
    if (node === root || root.contains(node)) return true;
    if (node.closest(PORTALED_OVERLAY_SELECTOR)) return true;
  }

  return false;
}

import { type RefObject, useEffect } from "react";
import { shouldIgnoreOutsideDismiss } from "./portaledOverlay";

/**
 * Close an open popup on Escape or on a click outside it.
 *
 * This effect was copy-pasted into nine components, and the copies had already
 * diverged — most listened for `mousedown` in the capture phase, one in the
 * bubble phase, and they disagreed on whether the key listener went on
 * `document` or `window`. Capture phase is the correct default: it sees the
 * event before a portaled menu can stop it, which is what
 * `shouldIgnoreOutsideDismiss` needs to decide whether the click landed inside
 * an overlay belonging to this popup.
 *
 * A press on `triggerRef` is left to the trigger's own click, which toggles
 * the popup. Closing here as well would let that click open it again.
 */
export function useDismissable(
  open: boolean,
  rootRef: RefObject<HTMLElement | null>,
  onDismiss: () => void,
  triggerRef?: RefObject<HTMLElement | null>,
): void {
  useEffect(() => {
    if (!open) return;

    const onPointerDown = (e: MouseEvent) => {
      if (shouldIgnoreOutsideDismiss(e, rootRef.current)) return;
      const trigger = triggerRef?.current;
      if (trigger && e.target instanceof Node && trigger.contains(e.target)) return;
      onDismiss();
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onDismiss();
    };

    document.addEventListener("mousedown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open, rootRef, onDismiss, triggerRef]);
}

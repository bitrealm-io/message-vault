import { type KeyboardEvent, type RefObject, useCallback, useEffect, useRef } from "react";
import { useDismissable } from "./useDismissable";

/** Roles that count as a focus stop inside a menu. */
const ITEM_SELECTOR = '[role="menuitem"], [role="menuitemradio"], [role="menuitemcheckbox"]';

/**
 * Keyboard behaviour for a popup that claims `role="menu"`: focus moves into it
 * on open, arrows and Home/End walk the items, Escape and Tab close it, and
 * focus returns to the trigger afterwards. Outside-click dismissal comes along
 * with it.
 *
 * The menus in this app declared the role (or `aria-haspopup="menu"` on their
 * trigger) without any of this, so they were reachable only with a pointer.
 */
export function useMenuKeyboard(
  open: boolean,
  rootRef: RefObject<HTMLElement | null>,
  onClose: () => void,
  triggerRef?: RefObject<HTMLElement | null>,
): { onKeyDown: (e: KeyboardEvent) => void } {
  useDismissable(open, rootRef, onClose, triggerRef);

  const wasOpenRef = useRef(false);

  useEffect(() => {
    if (open) {
      wasOpenRef.current = true;
      rootRef.current?.querySelector<HTMLElement>(ITEM_SELECTOR)?.focus();
      return;
    }
    if (wasOpenRef.current) {
      wasOpenRef.current = false;
      triggerRef?.current?.focus();
    }
  }, [open, rootRef, triggerRef]);

  const items = useCallback(
    () =>
      Array.from(rootRef.current?.querySelectorAll<HTMLElement>(ITEM_SELECTOR) ?? []).filter(
        (el) => !(el as HTMLButtonElement).disabled,
      ),
    [rootRef],
  );

  const move = useCallback(
    (delta: number) => {
      const rows = items();
      if (rows.length === 0) return;
      const at = rows.indexOf(document.activeElement as HTMLElement);
      rows[at < 0 ? 0 : (at + delta + rows.length) % rows.length]?.focus();
    },
    [items],
  );

  const onKeyDown = useCallback(
    (e: KeyboardEvent) => {
      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          move(1);
          break;
        case "ArrowUp":
          e.preventDefault();
          move(-1);
          break;
        case "Home": {
          e.preventDefault();
          items()[0]?.focus();
          break;
        }
        case "End": {
          e.preventDefault();
          const rows = items();
          rows[rows.length - 1]?.focus();
          break;
        }
        case "Tab":
          // Tabbing out of a menu closes it, as the pattern expects.
          onClose();
          break;
      }
    },
    [items, move, onClose],
  );

  return { onKeyDown };
}

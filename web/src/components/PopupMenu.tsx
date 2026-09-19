import { type ReactNode, type RefObject, useRef } from "react";
import { popupShadow } from "../lib/uiStyles";
import { useMenuKeyboard } from "../lib/useMenuKeyboard";

export type PopupMenuItem = {
  /** Stable key and default accessible name. */
  label: string;
  onSelect: () => void;
  disabled?: boolean;
  /** Replaces `label` in the rendered row when the row needs more than text. */
  children?: ReactNode;
  danger?: boolean;
};

const ITEM_CLASS =
  "block w-full cursor-pointer border-none bg-transparent px-3 py-1.5 text-left text-[0.813rem] text-text outline-none hover:bg-hover focus-visible:bg-hover disabled:cursor-not-allowed disabled:opacity-40";

/**
 * A menu popup with the keyboard behaviour the role implies.
 *
 * The hand-rolled menus this replaces declared `aria-haspopup="menu"` on their
 * triggers, but several rendered a plain `<div>` of `<button>`s underneath with
 * no `role="menu"` at all, and none of them moved focus into the popup,
 * responded to arrow keys, or put focus back on the trigger when they closed.
 *
 * Positioning stays with the caller — these menus are absolutely positioned
 * against a row or toolbar that already establishes the containing block.
 */
export default function PopupMenu({
  open,
  onClose,
  triggerRef,
  label,
  items,
  header,
  className = "",
}: {
  open: boolean;
  onClose: () => void;
  /** Focus returns here when the menu closes. */
  triggerRef?: RefObject<HTMLElement | null>;
  /** Accessible name for the menu itself. */
  label: string;
  items: PopupMenuItem[];
  /** Read-only block above the items, such as who is signed in. Not a focus stop. */
  header?: ReactNode;
  className?: string;
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const { onKeyDown } = useMenuKeyboard(open, rootRef, onClose, triggerRef);

  if (!open) return null;

  return (
    <div
      ref={rootRef}
      role="menu"
      aria-label={label}
      data-mv-overlay=""
      onKeyDown={onKeyDown}
      className={`min-w-[7.5rem] rounded-lg border border-border bg-popover py-1 ${popupShadow} ${className}`}
    >
      {header ? (
        <div className="mb-1 border-b border-border px-3 pt-1 pb-2 text-[0.813rem] text-text">
          {header}
        </div>
      ) : null}
      {items.map((item) => (
        <button
          key={item.label}
          type="button"
          role="menuitem"
          disabled={item.disabled}
          onClick={() => {
            onClose();
            item.onSelect();
          }}
          className={`${ITEM_CLASS} ${item.danger ? "!text-danger" : ""}`}
        >
          {item.children ?? item.label}
        </button>
      ))}
    </div>
  );
}

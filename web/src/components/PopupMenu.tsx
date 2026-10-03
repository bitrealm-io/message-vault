import type { ReactElement, ReactNode } from "react";
import { Menu, MenuItem, MenuTrigger, Popover, type PopoverProps } from "react-aria-components";
import { menuItemClass, menuPopoverClass } from "../lib/uiStyles";

export type PopupMenuItem = {
  /** Stable key and default accessible name. */
  label: string;
  onSelect: () => void;
  disabled?: boolean;
  /** Replaces `label` in the rendered row when the row needs more than text. */
  children?: ReactNode;
  danger?: boolean;
};

/**
 * A button that opens a menu: React Aria's `MenuTrigger`, `Popover` and `Menu`.
 *
 * React Aria gives the menu what the pattern expects: focus moves into it on
 * open, the arrow keys, Home, End and typing a letter move between items,
 * Enter or a click runs an item and closes the menu, Escape or a press outside
 * closes it, and focus goes back to the trigger.
 *
 * `trigger` must be a React Aria button (`Button` or `PlainButton`), which the
 * menu wires up: `aria-haspopup`, `aria-expanded`, and opening on press or
 * with the arrow keys. `aria-expanded` is the hook for a trigger's open look.
 */
export default function PopupMenu({
  trigger,
  label,
  items,
  header,
  placement = "bottom end",
  className = "",
}: {
  trigger: ReactElement;
  /** Accessible name for the menu itself. */
  label: string;
  items: PopupMenuItem[];
  /** Read-only block above the items, such as who is logged in. Not a focus stop. */
  header?: ReactNode;
  placement?: PopoverProps["placement"];
  /** Extra classes for the popover. */
  className?: string;
}) {
  return (
    <MenuTrigger>
      {trigger}
      <Popover
        placement={placement}
        offset={2}
        data-mc-overlay=""
        className={`${menuPopoverClass} ${className}`}
      >
        {header ? (
          <div className="mb-1 border-b border-border px-3 pt-1 pb-2 text-[0.813rem] text-text">
            {header}
          </div>
        ) : null}
        <Menu aria-label={label} shouldFocusWrap className="outline-none">
          {items.map((item) => (
            <MenuItem
              key={item.label}
              id={item.label}
              textValue={item.label}
              isDisabled={item.disabled}
              onAction={item.onSelect}
              className={`${menuItemClass} ${item.danger ? "text-danger" : "text-text"}`}
            >
              {item.children ?? item.label}
            </MenuItem>
          ))}
        </Menu>
      </Popover>
    </MenuTrigger>
  );
}

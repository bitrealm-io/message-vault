import { type ReactNode, useRef } from "react";
import { Checkbox as RACCheckbox } from "react-aria-components";

/**
 * The app's checkbox: React Aria's `Checkbox`, drawn as the compact list box
 * (`.mc-list-check` in `theme.css`).
 *
 * React Aria renders a `<label>` holding a visually hidden `<input>`, so the
 * box itself never takes focus. It marks the label instead, and the box draws
 * from those marks: `data-selected`, `data-indeterminate`, `data-disabled`
 * and, for the focus ring, `data-focus-visible`.
 *
 * Pass `children` for a visible label; otherwise `aria-label` is required, since
 * a checkbox with neither announces as an unnamed control.
 */
export type CheckboxProps = {
  checked: boolean;
  /** Mixed state — some but not all of the things this box covers are checked. */
  indeterminate?: boolean;
  /** `shiftKey` is there for a caller that checks a range on Shift + click. */
  onChange: (checked: boolean, modifiers: { shiftKey: boolean }) => void;
  disabled?: boolean;
  /** Extra classes for the drawn box. */
  className?: string;
  /** Extra classes for the label around the box (and `children`, when given). */
  labelClassName?: string;
  children?: ReactNode;
} & ({ children: ReactNode } | { "aria-label": string });

export default function Checkbox({
  checked,
  indeterminate = false,
  onChange,
  disabled,
  className = "",
  labelClassName = "",
  children,
  ...rest
}: CheckboxProps) {
  // React Aria's onChange carries no event, so the Shift key is read when the press starts.
  const shiftRef = useRef(false);
  const labelClass =
    children === undefined
      ? "inline-flex cursor-pointer data-disabled:cursor-not-allowed"
      : "inline-flex cursor-pointer items-center gap-2 text-[0.813rem] text-text data-disabled:cursor-not-allowed";

  return (
    <RACCheckbox
      {...rest}
      isSelected={checked}
      isIndeterminate={indeterminate && !checked}
      isDisabled={disabled}
      onPressStart={(e) => {
        shiftRef.current = e.shiftKey;
      }}
      onChange={(next) => {
        onChange(next, { shiftKey: shiftRef.current });
        shiftRef.current = false;
      }}
      className={`mc-check ${labelClass} ${labelClassName}`}
    >
      <span aria-hidden className={`mc-list-check shrink-0 ${className}`} />
      {children}
    </RACCheckbox>
  );
}

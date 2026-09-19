import { type ChangeEvent, type ReactNode, useId } from "react";

/**
 * The app's checkbox. Wraps the one raw `<input type="checkbox">` the codebase
 * should have, so callers stop hand-rolling the `indeterminate` ref assignment
 * (it has no HTML attribute and can only be set on the DOM node) and stop
 * picking a different set of utility classes at each call site.
 *
 * Pass `children` for a visible label; otherwise `aria-label` is required, since
 * a checkbox with neither announces as an unnamed control.
 */
export type CheckboxProps = {
  checked: boolean;
  /** Mixed state — some but not all of the things this box covers are checked. */
  indeterminate?: boolean;
  /** The event is there for a caller that reads a modifier key, such as Shift for a range. */
  onChange: (checked: boolean, event: ChangeEvent<HTMLInputElement>) => void;
  disabled?: boolean;
  /**
   * Set when an outer element needs to point a `htmlFor` at this input — a row
   * whose whole avatar area should toggle it, for instance.
   */
  id?: string;
  /** Extra classes for the input itself. */
  className?: string;
  /** Extra classes for the wrapping label, when `children` is given. */
  labelClassName?: string;
  children?: ReactNode;
} & ({ children: ReactNode } | { "aria-label": string });

export default function Checkbox({
  checked,
  indeterminate = false,
  onChange,
  disabled,
  id,
  className = "",
  labelClassName = "",
  children,
  ...rest
}: CheckboxProps) {
  const generatedId = useId();
  const inputId = id ?? generatedId;

  if (children === undefined) {
    return (
      <input
        type="checkbox"
        id={inputId}
        checked={checked}
        disabled={disabled}
        ref={(el) => {
          // `indeterminate` is DOM-only: React will not set it from a prop.
          if (el) el.indeterminate = indeterminate && !checked;
        }}
        onChange={(e) => onChange(e.target.checked, e)}
        className={`mv-list-check disabled:opacity-40 ${className}`}
        {...rest}
      />
    );
  }

  return (
    <label
      htmlFor={inputId}
      className={`inline-flex cursor-pointer items-center gap-2 text-[0.813rem] text-text ${labelClassName}`}
    >
      <input
        type="checkbox"
        id={inputId}
        checked={checked}
        disabled={disabled}
        ref={(el) => {
          if (el) el.indeterminate = indeterminate && !checked;
        }}
        onChange={(e) => onChange(e.target.checked, e)}
        className={`mv-list-check disabled:opacity-40 ${className}`}
        {...rest}
      />
      {children}
    </label>
  );
}

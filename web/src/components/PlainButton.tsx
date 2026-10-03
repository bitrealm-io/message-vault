import type { Ref } from "react";
import { Button as RACButton, type ButtonProps as RACButtonProps } from "react-aria-components";

export type PlainButtonProps = RACButtonProps & {
  /** Hover text. React Aria's Button drops `title`, so it is set on the element here. */
  title?: string;
  ref?: Ref<HTMLButtonElement>;
};

/**
 * React Aria's `Button` with no look of its own, for a control its caller draws
 * completely: a nav row, a glyph, a chip. `Button` is the one with the app's
 * variants and sizes.
 *
 * Every button in `web/src/` that runs an action is one of the two, so press,
 * hover, focus-visible and disabled behave the same everywhere
 * (`web/STYLE_GUIDE.md`, rule 4). A button that stays pressed, such as Find,
 * is React Aria's `ToggleButton`.
 *
 * A focus ring uses `focus-visible:`, because the button itself takes focus.
 */
export default function PlainButton({ title, ref, ...rest }: PlainButtonProps) {
  return (
    <RACButton
      {...rest}
      ref={(el) => {
        if (el && el.title !== (title ?? "")) el.title = title ?? "";
        if (typeof ref === "function") ref(el);
        else if (ref) ref.current = el;
      }}
    />
  );
}

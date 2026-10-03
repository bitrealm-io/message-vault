import type { ReactNode } from "react";
import PlainButton, { type PlainButtonProps } from "./PlainButton";

/**
 * 24px (`size-6`) round hit target for left-nav plus, ellipsis, and delete.
 * Trailing column width in `navSectionLayout` must stay 1.5rem to match.
 *
 * A menu trigger keeps its hover circle while its menu is open through
 * `aria-expanded`, which React Aria's `MenuTrigger` sets.
 */
export default function NavGlyphButton({
  children,
  danger = false,
  disabled,
  className = "",
  ...rest
}: Omit<PlainButtonProps, "className" | "children"> & {
  children: ReactNode;
  /** Trash control: keep the circle, use the danger color on the glyph. */
  danger?: boolean;
  disabled?: boolean;
  className?: string;
}) {
  const hoverText = danger
    ? "hover:text-danger focus-visible:text-danger"
    : "hover:text-text focus-visible:text-text";
  return (
    <PlainButton
      {...rest}
      isDisabled={disabled ?? rest.isDisabled}
      className={`box-border flex size-6 shrink-0 cursor-pointer items-center justify-center rounded-full border-none bg-transparent p-0 text-muted hover:bg-hover focus-visible:bg-hover disabled:cursor-default disabled:opacity-40 aria-expanded:bg-hover aria-expanded:text-text aria-expanded:opacity-100 ${hoverText} ${className}`.trim()}
    >
      {children}
    </PlainButton>
  );
}

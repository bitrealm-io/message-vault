import PlainButton, { type PlainButtonProps } from "./PlainButton";

export type ButtonVariant =
  | "primary"
  | "secondary"
  | "danger"
  | "ghost"
  | "ghostNeutral"
  | "ghostDanger";

/**
 * Sizes exist so call sites stop restating padding and type size as `!`-prefixed
 * overrides. Tailwind gives every utility the same specificity, so a class
 * passed in `className` does not reliably beat one baked into the component —
 * which is why those overrides all needed `!important` to work at all.
 */
export type ButtonSize = "wide" | "md" | "sm" | "xs" | "chip" | "icon";

const VARIANT: Record<ButtonVariant, string> = {
  primary: "bg-accent text-sent-text border-accent font-bold hover:brightness-90",
  secondary: "bg-elevated text-text border-border font-semibold hover:bg-hover",
  danger:
    "bg-danger-soft-bg text-danger border-danger-soft-border font-semibold hover:brightness-95",
  ghost: "bg-transparent text-accent border-transparent font-medium hover:bg-hover",
  /** Quiet until hovered, then picks up a chip — row edit actions. */
  ghostNeutral:
    "bg-transparent text-muted border-transparent font-normal hover:border-border hover:bg-elevated hover:text-text data-hovered:border-border data-hovered:bg-elevated data-hovered:text-text data-pressed:border-border data-pressed:bg-hover disabled:pointer-events-none",
  /** Quiet until hovered, then reads as destructive — row delete actions. */
  ghostDanger:
    "bg-transparent text-muted border-transparent font-normal hover:border-danger-soft-border hover:bg-danger-soft-bg hover:text-danger data-hovered:border-danger-soft-border data-hovered:bg-danger-soft-bg data-hovered:text-danger data-pressed:border-danger-soft-border data-pressed:bg-danger-soft-bg data-pressed:text-danger",
};

const SIZE: Record<ButtonSize, string> = {
  /** Primary action at the foot of a form or job panel. */
  wide: "px-6 py-2 text-[0.875rem]",
  md: "px-4 py-2 text-[0.875rem]",
  sm: "px-3 py-1.5 text-[0.813rem]",
  xs: "px-3 py-1 text-[0.813rem]",
  /** Inline control beside body text, e.g. the selection-count clear action. */
  chip: "px-2.5 py-1 text-[0.75rem]",
  /** Square control sized for a single glyph, used in table rows and toolbars. */
  icon: "aspect-square h-7 w-7 min-h-7 min-w-7 shrink-0 rounded-sm p-0 text-[0.813rem] leading-none",
};

export default function Button({
  variant = "secondary",
  size = "md",
  children,
  disabled,
  isDisabled,
  className,
  ...rest
}: Omit<PlainButtonProps, "className"> & {
  variant?: ButtonVariant;
  size?: ButtonSize;
  className?: string;
  disabled?: boolean;
}) {
  return (
    <PlainButton
      {...rest}
      isDisabled={disabled ?? isDisabled}
      className={`
        box-border inline-flex cursor-pointer select-none items-center justify-center
        rounded-md text-center leading-[1.25]
        border border-solid font-[inherit]
        outline-none
        focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-1
        disabled:cursor-not-allowed disabled:brightness-[0.72]
        ${SIZE[size]}
        ${VARIANT[variant]}
        ${className ?? ""}
      `}
    >
      {children}
    </PlainButton>
  );
}

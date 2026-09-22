/** The vault's messages start lowercase; a line of UI text should not. */
function sentenceCase(text: string): string {
  return text ? text.charAt(0).toUpperCase() + text.slice(1) : text;
}

/**
 * Error line for an auth form. The band is a reserved, fixed height and is
 * transparent when empty, so the surrounding form does not shift when a
 * message appears — an unbounded message would push what sits below it down
 * the card, and the card with it.
 *
 * The message sits at the *foot* of that band, so a one-line message lands
 * directly above whatever closes the card and a message long enough to wrap
 * grows upward into the space above rather than downward into the rule. The
 * right padding is the width of a scrollbar, so a message that scrolls does
 * not run underneath its own thumb.
 */
export default function AuthErrorFooter({
  error,
  className,
}: {
  error: string;
  /** Overrides the height of the reserved band, e.g. to allow more wrapping. */
  className?: string;
}) {
  return (
    <div
      className={`mb-2 flex flex-col overflow-y-auto pr-2 text-[0.813rem] leading-[1.35] ${className ?? "h-9"}`}
      style={{ color: error ? "var(--danger)" : "transparent" }}
      aria-live="polite"
    >
      {/* `mt-auto` rather than `justify-end`: it settles the message on the
          bottom edge just the same, but a message too long for the band still
          scrolls, where `justify-end` would push the overflow out through a top
          edge no scrollbar can reach. */}
      <div className="mt-auto">{sentenceCase(error) || " "}</div>
    </div>
  );
}

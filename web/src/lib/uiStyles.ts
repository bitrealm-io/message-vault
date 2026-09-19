/** Shared theme-aware Tailwind class strings using the tokens from theme.css. */

/**
 * The card itself never scrolls or resizes — but at 560px tall, a viewport
 * shorter than that (phone in landscape, a small desktop window) cannot fit a
 * vertically-centered flex container without its top overflowing off-screen
 * and unreachable. `overflow-y-auto` keeps the card centered on a normal
 * viewport while letting the *page* scroll to reach it on a short one.
 *
 * `box-border` is what keeps a normal viewport free of a scrollbar. theme.css
 * leaves out Tailwind's preflight, so there is no global `border-box` reset and
 * every box opts in by hand. Without it this one measured `100vh` of content
 * *plus* its own 32px of padding, so the page stood 32px taller than the window
 * it was drawn in and the browser fitted a scrollbar it had no use for.
 */
export const pageCenter =
  "min-h-screen box-border flex items-center justify-center bg-bg p-4 overflow-y-auto";
/**
 * Every auth card is the same 448 × 608 box on every screen and in every
 * state — it never resizes and never scrolls, so nothing moves underneath the
 * user as they step through login and setup. The height is set by the
 * tallest card: Create Account, whose three fields sit above the pinned
 * action row with room for a two-line error above it.
 */
export const authCard =
  "box-border flex h-[38rem] w-full max-w-md flex-col bg-panel border border-border rounded-lg shadow-[0_4px_24px_rgba(0,0,0,0.15)] p-8";

/** Content region of an auth card: everything above the pinned action row. */
export const authCardBody = "flex min-h-0 flex-1 flex-col";

/**
 * Action row pinned to the bottom of the frame: the primary action does not
 * move when switching between the Login and Create Account tabs. Profile
 * setup uses the same pinned footer for its own submit and back button, so
 * the action still lands in the same place on that screen too — it just
 * carries more below it there.
 */
export const authCardFooter = "mt-auto flex flex-col";
/**
 * Heading of a step inside the flow, set left rather than centred. `m-0` for
 * the same reason as `authScreenTitle`: without Tailwind's preflight a heading
 * keeps the browser's own margins, which push it off the top of the card and
 * away from the line under it.
 */
export const authTitle = "m-0 text-[1.25rem] font-bold text-text mb-6 text-left";
/**
 * Name at the top of an auth card. The login card and the vault settings
 * screen share it, so crossing between them never changes the size of the
 * words at the top of the frame. `m-0` because theme.css leaves out Tailwind's
 * preflight, so a heading still carries the browser's own margins otherwise;
 * each screen sets the gap below the name itself.
 */
export const authScreenTitle =
  "m-0 text-center text-[1.375rem] font-semibold tracking-[-0.015em] text-text";
export const authLabel = "block text-[0.875rem] font-medium text-text mb-1";
export const authInput =
  "w-full box-border px-3 py-2 text-[0.875rem] rounded border border-border bg-elevated text-text focus:outline-none focus:border-accent";
export const mutedText = "text-[0.813rem] text-muted";
export const accentLink =
  "text-[0.813rem] text-accent cursor-pointer bg-transparent border-none p-0 hover:underline";

/** Floating panels / menus (advanced search, selects, date pickers, recent searches). */
export const popupShadow = "shadow-[0_10px_32px_rgba(0,0,0,0.28),0_2px_8px_rgba(0,0,0,0.14)]";

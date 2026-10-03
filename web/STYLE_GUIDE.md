# Web Frontend Style Guide

## Theme System

The four-seed theme system uses CSS custom properties:

- `--header` — header bar background
- `--accent` — interactive accent color, focus rings
- `--bg` — page canvas background
- `--panel` — content container backgrounds

These drive a `color-mix` derivation tree in `theme.css`. Three `data-theme` modes: `light`, `dark`, and custom (user-set header/accent values). All components reference Tailwind utility classes that map to these tokens — **never hardcode colors in components.**

### Token → Utility Mapping

| CSS Variable | Tailwind Utility |
|---|---|
| `--bg` | `bg-bg` |
| `--panel` | `bg-panel` |
| `--sidebar` | `bg-sidebar` |
| `--elevated` | `bg-elevated` |
| `--popover` | `bg-popover` |
| `--border` | `border-border` |
| `--text` | `text-text` |
| `--muted` | `text-muted` |
| `--accent` | `bg-accent`, `text-accent`, `ring-accent` |
| `--sent` | `bg-sent` |
| `--sent-text` | `text-sent-text` |
| `--received` | `bg-received` |
| `--received-text` | `text-received-text` |
| `--hover` | `bg-hover` |
| `--scrim` | `bg-scrim` |
| `--danger` | `text-danger` |
| `--danger-soft-bg` | `bg-danger-soft-bg` |
| `--danger-soft-border` | `border-danger-soft-border` |
| `--ok` | `text-ok` |
| `--ok-soft-bg` | `bg-ok-soft-bg` |
| `--lightbox-bg`, `--lightbox-control`, `--lightbox-text` | `bg-lightbox-bg`, `bg-lightbox-control`, `text-lightbox-text` |
| `--avatar-1` … `--avatar-8`, `--avatar-text` | `bg-avatar-1` … `bg-avatar-8` (picked by `contactAvatarClass`), `text-avatar-text` |
| `--elevation-card`, `-popup`, `-modal`, `-drawer`, `-contact-drawer`, `-pill`, `-bubble` | `shadow-card`, `shadow-popup`, `shadow-modal`, `shadow-drawer`, `shadow-contact-drawer`, `shadow-pill`, `shadow-bubble` |
| etc. | (see `@theme inline` in `theme.css` for full list) |

## Surface Layers (z-index not CSS z-index)

| Layer | Utility | Usage |
|---|---|---|
| Canvas | `bg-bg` | Page background |
| Sidebar | `bg-sidebar` | Navigation panels |
| Content | `bg-panel` | Cards, content blocks |
| Elevated | `bg-elevated` | Form inputs, buttons |
| Popover | `bg-popover` | Dropdowns, popups |

## Typography

- Section labels: `text-[0.688rem] font-semibold uppercase tracking-[0.05em] text-muted` (12px)
- Body: `text-[0.813rem]`—`text-[0.875rem]` (13–14px)
- Font: `system-ui, -apple-system, sans-serif`

## Interaction Patterns

- **Hover:** `hover:bg-hover` or `hover:brightness-*`
- **Focus:** `outline-none` (custom) + `focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-1`.
  The variant depends on which element takes focus.
  A button, a menu item, a table row or a list row takes DOM focus itself, so its ring uses `focus-visible:`.
  React Aria's `Checkbox`, `Radio` and `ColumnResizer` put focus on a hidden input, which `:focus-visible` cannot style, so their ring uses `data-focus-visible:` on the label React Aria marks (or the `isFocusVisible` render prop).
- **Current item in a menu or popdown:** `data-focused:bg-hover`. React Aria moves focus to the item under the pointer as well as the one the arrow keys reach, so one item is highlighted at a time.
- **Disabled:** `disabled:opacity-50` or `disabled:brightness-[0.72]` + `disabled:cursor-not-allowed`
- **Active/Selected:** `bg-accent text-sent-text`

## Overlay Z-Index Ladder

Defined as named constants in `src/lib/zLayers.ts`. Use those rather than a bare
`z-[…]`, and add a rung there if none of these fit.

| z-index | Constant | Usage |
|---|---|---|
| 1 | `Z_LIFT` | One step above siblings in the same stacking context (a row's lead cell, a table's column grip) |
| 10 | `Z_RANGE_PILL` | The floating range pill over a list's rows |
| 20 | `Z_APP_HEADER` | The app header, so its menus paint over the panels below |
| 30 | `Z_RESIZE_HANDLE` | Column resize handles. They sit below every drawer and panel opened over them. |
| 35 | `Z_CONTACT_DRAWER` | The overlay contact drawer. It is not modal, so it sits above the handles and below every scrim. |
| 40 | `Z_DRAWER_SCRIM` | Drawer scrim |
| 50 | `Z_DRAWER` | Modal drawer panel (the Sources panel) |
| 70 | `Z_INLINE_PANEL` | Inline overlays, such as the advanced search panel. They clear the resize handle. |
| 71 | `Z_INLINE_PANEL_TAIL` | The pointer tail on an inline panel |
| 100 | `Z_POPOVER` | Select/ComboBox popovers, menus, the contact-search popdown |
| 200 | `Z_MODAL` | Modal dialogs, lightbox |
| 250 | `Z_POPOVER_IN_MODAL` | A select popover opened inside a modal |

## Rules

1. **Tokens only.** No hex, `rgb()`/`rgba()` or Tailwind palette color (`text-white`) in component code, shadows included. Use Tailwind utilities that reference theme tokens, and add a token to `theme.css` when none fits. `src/styleTokens.test.ts` fails on a hex, `rgb()`/`hsl()` or palette color, a `z-` class or an inline `zIndex` written into `src/`. The theme presets in `lib/theme.ts` and the color picker in `components/theme/ThemeColorRow.tsx` may hold hex values, because those are colors a person picks, kept as data.
2. **No global border-box.** Some controls rely on `content-box`. Converted controls opt in with `box-border`.
3. **Compact density.** Keep the existing compact visual density — 13–14px body text, tight padding.
4. **React Aria for interactivity.** All interactive components (buttons, selects, dialogs, tabs, etc.) use `react-aria-components` for accessibility. A button is `Button` (the app's variants and sizes) or `PlainButton` (no look of its own), both React Aria's `Button`, and a button that stays pressed is React Aria's `ToggleButton`. Biome's `noRestrictedElements` refuses a native `<button>` outside tests. A checkbox is `Checkbox`, and a menu is `PopupMenu`.
5. **Inline styles only for dynamic values.** Layout math (VirtualList), dynamic widths, positions — keep as `style={{}}`. Static values → Tailwind className.

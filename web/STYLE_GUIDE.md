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
- **Focus:** `outline-none` (custom) + `focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-1`
- **Disabled:** `disabled:opacity-50` or `disabled:brightness-[0.72]` + `disabled:cursor-not-allowed`
- **Active/Selected:** `bg-accent text-sent-text`

## Overlay Z-Index Ladder

Defined as named constants in `src/lib/zLayers.ts`. Use those rather than a bare
`z-[…]`, and add a rung there if none of these fit.

| z-index | Constant | Usage |
|---|---|---|
| 40 | `Z_DRAWER_SCRIM` | Drawer scrim |
| 50 | `Z_DRAWER` | Drawer panel |
| 60 | `Z_RESIZE_HANDLE` | Column resize handles |
| 70 | `Z_INLINE_PANEL` | Inline overlays (advanced search panel) — clears the resize handle |
| 71 | `Z_INLINE_PANEL_TAIL` | The pointer tail on an inline panel |
| 100 | `Z_POPOVER` | Select/ComboBox popovers, menus, the contact-search popdown |
| 200 | `Z_MODAL` | Modal dialogs, lightbox |
| 250 | `Z_POPOVER_IN_MODAL` | A select popover opened inside a modal |

## Rules

1. **Tokens only.** No raw hex colors in component code. Use Tailwind utilities that reference theme tokens.
2. **No global border-box.** Some controls rely on `content-box`. Converted controls opt in with `box-border`.
3. **Compact density.** Keep the existing compact visual density — 13–14px body text, tight padding.
4. **React Aria for interactivity.** All interactive components (buttons, selects, dialogs, tabs, etc.) use `react-aria-components` for accessibility.
5. **Inline styles only for dynamic values.** Layout math (VirtualList), dynamic widths, positions — keep as `style={{}}`. Static values → Tailwind className.

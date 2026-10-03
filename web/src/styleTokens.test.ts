import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { RANGE_PILL_SCROLL_PAD_CLASS, RANGE_PILL_SPACER_CLASS } from "./components/ListRangePill";
import { AVATAR_COLOR_CLASSES } from "./lib/contactInitials";
import { Z_CONTACT_DRAWER, Z_DRAWER_SCRIM, Z_MODAL, Z_RESIZE_HANDLE } from "./lib/zLayers";

// The style guide's rules (STYLE_GUIDE.md, "Rules" 1 and "Overlay Z-Index
// Ladder"): colors come from theme.css tokens so the light and dark themes stay
// in one place, and z-index values come from the ladder in lib/zLayers.ts so
// overlays stack the same way everywhere.

const SRC = new URL("./", import.meta.url);
const themeCss = readFileSync(new URL("theme.css", SRC), "utf8");

/** Every .ts/.tsx source under src/, tests left out, as [path, text]. */
function sources(): [string, string][] {
  return readdirSync(SRC, { recursive: true, encoding: "utf8" })
    .filter((p) => /\.tsx?$/.test(p) && !/\.(test|spec)\.tsx?$/.test(p))
    .map((p) => [p.replaceAll("\\", "/"), readFileSync(new URL(p, SRC), "utf8")]);
}

/** Lines of `text` matching `re`, as "path:line: text". */
function hits(path: string, text: string, re: RegExp): string[] {
  return text
    .split("\n")
    .flatMap((line, i) => (re.test(line) ? [`${path}:${i + 1}: ${line.trim()}`] : []));
}

// The theme presets and the color picker hold colors as data a person picks,
// not as styling, so they are the only places a hex value may appear.
const HEX_DATA_FILES = new Set(["lib/theme.ts", "components/theme/ThemeColorRow.tsx"]);

describe("colors are theme tokens", () => {
  it("no component writes a hex color, an rgb() color or a palette color", () => {
    const hex = /["'`[]#[0-9a-fA-F]{3,8}\b/;
    const rgb = /(?<![a-zA-Z])(rgba?|hsla?)\(/;
    const palette =
      /\b(bg|text|border|ring|outline|fill|stroke|shadow|from|to|via|decoration)-(white|black|(slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose)-\d{2,3})\b/;
    const found = sources().flatMap(([path, text]) => [
      ...(HEX_DATA_FILES.has(path) ? [] : hits(path, text, hex)),
      ...hits(path, text, rgb),
      ...hits(path, text, palette),
    ]);
    expect(found).toEqual([]);
  });

  it("every token the Tailwind map names is defined", () => {
    const themeBlock = themeCss.slice(themeCss.indexOf("@theme inline"));
    const referenced = [...themeBlock.matchAll(/--(?:color|shadow)-[\w-]+:\s*var\((--[\w-]+)\)/g)]
      .map((m) => m[1])
      .filter((name): name is string => name != null);
    expect(referenced.length).toBeGreaterThan(0);
    const missing = referenced.filter((name) => !themeCss.includes(`  ${name}:`));
    expect(missing).toEqual([]);
  });

  it("every avatar color class has a token", () => {
    for (const cls of AVATAR_COLOR_CLASSES) {
      expect(themeCss).toContain(`--color-${cls.replace(/^bg-/, "")}:`);
    }
  });
});

describe("z-index values come from the ladder", () => {
  it("no source outside lib/zLayers.ts writes a z-index class", () => {
    const zClass = /(^|[\s"'`:!])-?z-(\d|\[)/;
    const found = sources()
      .filter(([path]) => path !== "lib/zLayers.ts")
      .flatMap(([path, text]) => hits(path, text, zClass));
    expect(found).toEqual([]);
  });

  // The overlay contact drawer is not modal and stays open while Sources or a
  // dialog opens; under the scrim, that dialog dims it and takes its clicks.
  it("the overlay contact drawer sits above the resize handles and below every scrim", () => {
    const rung = (cls: string) => Number(/^z-\[?(\d+)\]?$/.exec(cls)?.[1]);
    expect(rung(Z_CONTACT_DRAWER)).toBeGreaterThan(rung(Z_RESIZE_HANDLE));
    expect(rung(Z_CONTACT_DRAWER)).toBeLessThan(rung(Z_DRAWER_SCRIM));
    expect(rung(Z_CONTACT_DRAWER)).toBeLessThan(rung(Z_MODAL));
  });
});

describe("the range pill's room", () => {
  // A list leaves room under its last row either as padding or as a spacer;
  // the two must match, or one kind of list hides its last row under the pill.
  it("is the same size as padding and as a spacer", () => {
    expect(RANGE_PILL_SCROLL_PAD_CLASS.replace(/^pb-/, "")).toBe(
      RANGE_PILL_SPACER_CLASS.replace(/^h-/, ""),
    );
  });
});

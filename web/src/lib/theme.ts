import { readPref } from "./storage";

/** Light, dark, or follow the operating system color scheme. */
export type ThemeMode = "light" | "dark" | "system";

/** Light or dark after applying the system preference. */
export type ResolvedTheme = "light" | "dark";

/** Four colors that define a theme: header and accent for light and dark. */
export type ThemeSeeds = {
  lightHeader: string;
  lightAccent: string;
  darkHeader: string;
  darkAccent: string;
};

export type ThemePreset = {
  id: string;
  label: string;
  seeds: ThemeSeeds;
};

export const THEME_MODE_KEY = "mc-theme";
export const THEME_SEEDS_KEY = "mc-theme-seeds";

export const DEFAULT_MODE: ThemeMode = "dark";

/** Ocean Depths — theme-factory default. */
export const DEFAULT_SEEDS: ThemeSeeds = {
  lightHeader: "#f1faee",
  lightAccent: "#2d8b8b",
  darkHeader: "#1a2332",
  darkAccent: "#a8dadc",
};

export const THEME_PRESETS: ThemePreset[] = [
  { id: "ocean-depths", label: "Ocean Depths", seeds: DEFAULT_SEEDS },
  {
    id: "graphite-blue",
    label: "Graphite Blue",
    seeds: {
      lightHeader: "#e6e9ee",
      lightAccent: "#2b7fff",
      darkHeader: "#222426",
      darkAccent: "#5ea1ff",
    },
  },
  {
    id: "light",
    label: "Light",
    seeds: {
      lightHeader: "#f0f2f5",
      lightAccent: "#2563eb",
      darkHeader: "#2c3036",
      darkAccent: "#6ba3ff",
    },
  },
  {
    id: "dark",
    label: "Dark",
    seeds: {
      lightHeader: "#e8eaed",
      lightAccent: "#3b82f6",
      darkHeader: "#141618",
      darkAccent: "#5ea1ff",
    },
  },
  {
    id: "sunset-boulevard",
    label: "Sunset Boulevard",
    seeds: {
      lightHeader: "#e9c46a",
      lightAccent: "#e76f51",
      darkHeader: "#264653",
      darkAccent: "#f4a261",
    },
  },
  {
    id: "forest-canopy",
    label: "Forest Canopy",
    seeds: {
      lightHeader: "#faf9f6",
      lightAccent: "#7d8471",
      darkHeader: "#2d4a2b",
      darkAccent: "#a4ac86",
    },
  },
  {
    id: "modern-minimalist",
    label: "Modern Minimalist",
    seeds: {
      lightHeader: "#d3d3d3",
      lightAccent: "#708090",
      darkHeader: "#36454f",
      // Darken the gray so white text on sent bubbles stays readable.
      darkAccent: "#9aa8b5",
    },
  },
  {
    id: "golden-hour",
    label: "Golden Hour",
    seeds: {
      lightHeader: "#d4b896",
      lightAccent: "#c1666b",
      darkHeader: "#4a403a",
      darkAccent: "#f4a900",
    },
  },
  {
    id: "arctic-frost",
    label: "Arctic Frost",
    seeds: {
      lightHeader: "#fafafa",
      lightAccent: "#4a6fa5",
      darkHeader: "#4a6fa5",
      // Darken ice blue so white text on sent bubbles stays readable.
      darkAccent: "#5a7fb5",
    },
  },
  {
    id: "desert-rose",
    label: "Desert Rose",
    seeds: {
      lightHeader: "#e8d5c4",
      lightAccent: "#b87d6d",
      darkHeader: "#5d2e46",
      darkAccent: "#d4a5a5",
    },
  },
  {
    id: "tech-innovation",
    label: "Tech Innovation",
    seeds: {
      lightHeader: "#ffffff",
      lightAccent: "#0066ff",
      darkHeader: "#1e1e1e",
      // Mix neon cyan toward blue so white text on sent bubbles stays readable.
      darkAccent: "#0088bb",
    },
  },
  {
    id: "botanical-garden",
    label: "Botanical Garden",
    seeds: {
      lightHeader: "#f5f3ed",
      lightAccent: "#b7472a",
      darkHeader: "#4a7c59",
      darkAccent: "#f9a620",
    },
  },
  {
    id: "midnight-galaxy",
    label: "Midnight Galaxy",
    seeds: {
      lightHeader: "#e6e6fa",
      lightAccent: "#4a4e8f",
      darkHeader: "#2b1e3e",
      darkAccent: "#a490c2",
    },
  },
];

const HEX_RE = /^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/;

/** True when the value is a saved appearance choice, including "follow the system". */
function isThemeMode(value: string | null | undefined): value is ThemeMode {
  return value === "light" || value === "dark" || value === "system";
}

/** Return a lowercase `#rrggbb` color, or null when the text is not a hex color. */
export function normalizeHex(raw: string): string | null {
  const t = raw.trim();
  if (!HEX_RE.test(t)) return null;
  if (t.length === 4) {
    const [, r, g, b] = t;
    return `#${r}${r}${g}${g}${b}${b}`.toLowerCase();
  }
  return t.toLowerCase();
}

/** Join the four theme colors into a shareable comma-separated string. */
export function formatThemeShare(seeds: ThemeSeeds): string {
  return [seeds.lightHeader, seeds.lightAccent, seeds.darkHeader, seeds.darkAccent]
    .map((h) => normalizeHex(h) ?? h)
    .join(",");
}

/** Parse four hex colors from a share string. Returns null when any color is invalid. */
export function parseThemeShare(raw: string): ThemeSeeds | null {
  const parts = raw
    .split(/[,\s]+/)
    .map((p) => p.trim())
    .filter(Boolean);
  if (parts.length !== 4) return null;
  const lightHeader = normalizeHex(parts[0]);
  const lightAccent = normalizeHex(parts[1]);
  const darkHeader = normalizeHex(parts[2]);
  const darkAccent = normalizeHex(parts[3]);
  if (!lightHeader || !lightAccent || !darkHeader || !darkAccent) return null;
  return { lightHeader, lightAccent, darkHeader, darkAccent };
}

function parseStoredSeeds(raw: string | null): ThemeSeeds | null {
  if (!raw) return null;
  const asShare = parseThemeShare(raw);
  if (asShare) return asShare;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return null;
    const obj = parsed as Record<string, unknown>;
    const lightHeader = typeof obj.lightHeader === "string" ? normalizeHex(obj.lightHeader) : null;
    const lightAccent = typeof obj.lightAccent === "string" ? normalizeHex(obj.lightAccent) : null;
    const darkHeader = typeof obj.darkHeader === "string" ? normalizeHex(obj.darkHeader) : null;
    const darkAccent = typeof obj.darkAccent === "string" ? normalizeHex(obj.darkAccent) : null;
    if (!lightHeader || !lightAccent || !darkHeader || !darkAccent) return null;
    return { lightHeader, lightAccent, darkHeader, darkAccent };
  } catch {
    return null;
  }
}

function prefersDarkScheme(): boolean {
  if (typeof window === "undefined") return true;
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

/** Pick light or dark from the saved mode and the system color-scheme setting. */
export function resolveMode(mode: ThemeMode, prefersDark = prefersDarkScheme()): ResolvedTheme {
  if (mode === "system") return prefersDark ? "dark" : "light";
  return mode;
}

function activeSeeds(
  seeds: ThemeSeeds,
  resolved: ResolvedTheme,
): { header: string; accent: string } {
  return resolved === "dark"
    ? { header: seeds.darkHeader, accent: seeds.darkAccent }
    : { header: seeds.lightHeader, accent: seeds.lightAccent };
}

/** Write the chosen mode and colors onto `<html>` as `data-theme` and CSS variables. */
export function applyTheme(mode: ThemeMode, seeds: ThemeSeeds): ResolvedTheme {
  const resolved = resolveMode(mode);
  const { header, accent } = activeSeeds(seeds, resolved);
  const root = document.documentElement;
  root.setAttribute("data-theme", resolved);
  root.style.setProperty("--header", header);
  root.style.setProperty("--accent", accent);
  return resolved;
}

/** Read the saved appearance mode, or the default when nothing is stored or storage is blocked. */
export function readStoredMode(): ThemeMode {
  const raw = readPref(THEME_MODE_KEY);
  if (isThemeMode(raw)) return raw;
  return DEFAULT_MODE;
}

/** Read the saved theme colors, or the default palette when nothing is stored or storage is blocked. */
export function readStoredSeeds(): ThemeSeeds {
  return parseStoredSeeds(readPref(THEME_SEEDS_KEY)) ?? DEFAULT_SEEDS;
}

// The first-paint theme script lives in web/index.html so the page is not
// the wrong colors for a frame before React starts.

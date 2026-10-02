import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { writePref } from "./storage";
import {
  applyTheme,
  DEFAULT_MODE,
  DEFAULT_SEEDS,
  formatThemeShare,
  parseThemeShare,
  type ResolvedTheme,
  readStoredMode,
  readStoredSeeds,
  resolveMode,
  THEME_MODE_KEY,
  THEME_PRESETS,
  THEME_SEEDS_KEY,
  type ThemeMode,
  type ThemePreset,
  type ThemeSeeds,
} from "./theme";

export type UseThemeResult = {
  mode: ThemeMode;
  setMode: (mode: ThemeMode) => void;
  seeds: ThemeSeeds;
  setSeeds: (seeds: ThemeSeeds) => void;
  patchSeed: (key: keyof ThemeSeeds, hex: string) => void;
  shareString: string;
  setShareString: (raw: string) => boolean;
  applyPreset: (preset: ThemePreset) => void;
  resolvedMode: ResolvedTheme;
  presets: ThemePreset[];
};

const ThemeContext = createContext<UseThemeResult | null>(null);

/** Shared theme state. Writes CSS variables on `<html>` and saves choices in the browser. */
export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setModeState] = useState<ThemeMode>(DEFAULT_MODE);
  const [seeds, setSeedsState] = useState<ThemeSeeds>(DEFAULT_SEEDS);
  const [prefersDark, setPrefersDark] = useState(true);
  // Wait until saved values load so the first-paint script is not overwritten
  // with defaults for a frame.
  const [hydrated, setHydrated] = useState(false);

  useEffect(() => {
    setModeState(readStoredMode());
    setSeedsState(readStoredSeeds());
    setHydrated(true);
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    setPrefersDark(mq.matches);
    const onChange = () => setPrefersDark(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  useEffect(() => {
    if (!hydrated) return;
    void prefersDark;
    applyTheme(mode, seeds);
  }, [hydrated, mode, seeds, prefersDark]);

  const setMode = useCallback((next: ThemeMode) => {
    setModeState(next);
    writePref(THEME_MODE_KEY, next);
  }, []);

  const setSeeds = useCallback((next: ThemeSeeds) => {
    setSeedsState(next);
    writePref(THEME_SEEDS_KEY, formatThemeShare(next));
  }, []);

  const patchSeed = useCallback((key: keyof ThemeSeeds, hex: string) => {
    setSeedsState((prev) => {
      const next = { ...prev, [key]: hex };
      writePref(THEME_SEEDS_KEY, formatThemeShare(next));
      return next;
    });
  }, []);

  const shareString = useMemo(() => formatThemeShare(seeds), [seeds]);

  const setShareString = useCallback((raw: string) => {
    const parsed = parseThemeShare(raw);
    if (!parsed) return false;
    setSeedsState(parsed);
    writePref(THEME_SEEDS_KEY, formatThemeShare(parsed));
    return true;
  }, []);

  const applyPreset = useCallback((preset: ThemePreset) => {
    setSeedsState(preset.seeds);
    writePref(THEME_SEEDS_KEY, formatThemeShare(preset.seeds));
  }, []);

  const resolvedMode = resolveMode(mode, prefersDark);

  const value = useMemo(
    () => ({
      mode,
      setMode,
      seeds,
      setSeeds,
      patchSeed,
      shareString,
      setShareString,
      applyPreset,
      resolvedMode,
      presets: THEME_PRESETS,
    }),
    [
      mode,
      setMode,
      seeds,
      setSeeds,
      patchSeed,
      shareString,
      setShareString,
      applyPreset,
      resolvedMode,
    ],
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

/** Current theme. Must be called under ThemeProvider. */
export function useTheme(): UseThemeResult {
  const ctx = useContext(ThemeContext);
  if (!ctx) {
    throw new Error("useTheme must be used within ThemeProvider");
  }
  return ctx;
}

import { readPref, writePref } from "../lib/storage";

/** Clamp a column width to [min, max], rounded to the nearest pixel. */
export function clampWidth(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, Math.round(n)));
}

/** Read a stored column width, or return defaultWidth when missing/invalid. */
export function loadWidth(
  storageKey: string,
  defaultWidth: number,
  min: number,
  max: number,
): number {
  const raw = readPref(storageKey);
  if (!raw) return defaultWidth;
  const n = Number(raw);
  if (!Number.isFinite(n)) return defaultWidth;
  return clampWidth(n, min, max);
}

/** Persist a column width. Ignores private-browsing / quota failures. */
export function saveWidth(storageKey: string, n: number): void {
  writePref(storageKey, String(n));
}

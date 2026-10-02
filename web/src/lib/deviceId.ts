import { newId } from "./newId";
import { readPref, writePref } from "./storage";

/** localStorage key for this install's stable identifier. */
export const DEVICE_ID_KEY = "mc-device-id";

let cached: string | null = null;

/**
 * Stable id for this install.
 *
 * A session records which install created it, so a different machine can
 * say where the staged work lives instead of failing to open a path that
 * was never local to it.
 *
 * Generated on first read and kept in localStorage. When storage is
 * unavailable the id lives only in memory for this page, which degrades
 * to "this looks like a different install after a reload" — the resume
 * screen handles that case rather than breaking.
 */
export function getDeviceId(): string {
  if (cached) return cached;
  const stored = readPref(DEVICE_ID_KEY)?.trim();
  if (stored) {
    cached = stored;
    return stored;
  }
  const fresh = newId();
  cached = fresh;
  writePref(DEVICE_ID_KEY, fresh);
  return fresh;
}

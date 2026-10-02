/**
 * The one way `web/src` reads and writes `localStorage`.
 *
 * A browser set to block site data throws `SecurityError` from the
 * `localStorage` getter itself, and a full or private-mode storage throws from
 * `setItem`. Every saved value here is a preference the app can do without, so
 * a blocked read gives `null` and a write that fails is dropped: the website
 * still opens, and a change applies for the visit without being saved.
 */

/** The saved value for `key`, or `null` when nothing is saved or storage is blocked. */
export function readPref(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** Save `value` under `key`. A blocked or full storage drops the write. */
export function writePref(key: string, value: string): void {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // Blocked or full storage: the value lasts only for this visit.
  }
}

/** Remove the value saved under `key`. A blocked storage leaves nothing to remove. */
export function removePref(key: string): void {
  try {
    window.localStorage.removeItem(key);
  } catch {
    // Blocked storage: nothing was saved there.
  }
}

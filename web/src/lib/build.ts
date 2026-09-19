import { isTauri } from "./tauri-check";

/**
 * This app's Build, such as `0.9.0+343fe0d8`. `vite.config.ts` works it out
 * when the bundle is made and replaces `__APP_BUILD__` with it, so a browser
 * tab left open across a vault upgrade goes on reporting the Build it loaded.
 */
export const APP_BUILD: string = __APP_BUILD__;

/** Which app this is, as the vault records it. */
export function appKind(): "desktop" | "website" {
  return isTauri() ? "desktop" : "website";
}

/**
 * The headers that name this app to the vault on every request. The vault
 * records them on the session and shows them to the vault owner; it serves the
 * request whatever they say.
 */
export function appHeaders(): Record<string, string> {
  return {
    "x-message-vault-app": appKind(),
    "x-message-vault-version": APP_BUILD,
  };
}

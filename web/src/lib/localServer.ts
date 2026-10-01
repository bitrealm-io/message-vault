import { invoke } from "@tauri-apps/api/core";
import { DEFAULT_TAURI_SERVER_URL } from "./authGuards";

/**
 * The Message Crate the desktop app starts for itself. The app ships the
 * server and runs it at its own address while the app is open; the rules are
 * in `src-tauri/src/local_server.rs`.
 */

/** Why the app's own Message Crate is not running. */
export type LocalServerFailure = "port_taken" | "start_failed";

/** What the desktop app reports about its own Message Crate. */
export type LocalServerStatus =
  | { status: "idle" }
  | { status: "starting"; first_time: boolean }
  | { status: "ready"; started_by_app: boolean }
  | { status: "failed"; reason: LocalServerFailure; message: string; details: string };

/**
 * Whether `url` is the app's own address, the one it starts a server for.
 * Any other address is a Message Crate the person chose, which the app never
 * starts.
 */
export function isOwnAddress(url: string): boolean {
  return url.trim().replace(/\/+$/, "") === DEFAULT_TAURI_SERVER_URL;
}

/**
 * Make sure the app's own Message Crate is running. Safe to call on every
 * launch: a Message Crate already answering is used as it is, and a start
 * under way is left alone. Calling it again after a failure tries again, and
 * calling it after the network setting changed restarts the app's own server.
 */
export async function startLocalServer(): Promise<LocalServerStatus> {
  return invoke<LocalServerStatus>("start_local_server", { openToNetwork: getOpenToNetwork() });
}

const OPEN_TO_NETWORK_KEY = "mc-local-server-open-to-network";

/**
 * Whether the app's own Message Crate accepts connections from other devices
 * on the network. Off unless the person switched it on: the connection is
 * plain HTTP.
 */
export function getOpenToNetwork(): boolean {
  try {
    return localStorage.getItem(OPEN_TO_NETWORK_KEY) === "1";
  } catch {
    return false;
  }
}

/**
 * Save the setting. It takes effect at the next `startLocalServer`, which
 * restarts the app's own server when it was started the other way.
 */
export function setOpenToNetwork(on: boolean): void {
  try {
    if (on) localStorage.setItem(OPEN_TO_NETWORK_KEY, "1");
    else localStorage.removeItem(OPEN_TO_NETWORK_KEY);
  } catch {
    // Private browsing and full storage can throw.
  }
}

/** Read the state of the app's own Message Crate without starting it. */
export async function localServerStatus(): Promise<LocalServerStatus> {
  return invoke<LocalServerStatus>("local_server_status");
}

/** Open the folder holding the app's own database and attachments. */
export async function openDataFolder(): Promise<void> {
  await invoke("open_data_folder");
}

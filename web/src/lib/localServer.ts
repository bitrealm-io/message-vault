import { invoke } from "@tauri-apps/api/core";
import { DEFAULT_TAURI_SERVER_URL } from "./authGuards";
import { readPref, removePref, writePref } from "./storage";

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
 * a Message Crate the app found is asked again whether it still answers.
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
  return readPref(OPEN_TO_NETWORK_KEY) === "1";
}

/**
 * Save the setting for the next launch. `setLocalServerOpenToNetwork` gives
 * it to a server the app runs now.
 */
export function setOpenToNetwork(on: boolean): void {
  if (on) writePref(OPEN_TO_NETWORK_KEY, "1");
  else removePref(OPEN_TO_NETWORK_KEY);
}

/**
 * Give the network setting to the desktop app. It starts nothing: the app's
 * own server is restarted to match, once no import or other desktop job
 * runs, and a Message Crate the app only found is left as it is.
 */
export async function setLocalServerOpenToNetwork(on: boolean): Promise<LocalServerStatus> {
  return invoke<LocalServerStatus>("set_open_to_network", { openToNetwork: on });
}

/** Read the state of the app's own Message Crate without starting it. */
export async function localServerStatus(): Promise<LocalServerStatus> {
  return invoke<LocalServerStatus>("local_server_status");
}

/** Open the folder holding the app's own database and attachments. */
export async function openDataFolder(): Promise<void> {
  await invoke("open_data_folder");
}

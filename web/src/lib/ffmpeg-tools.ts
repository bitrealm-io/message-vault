import { readPref } from "./storage";
import { isTauri } from "./tauri-check";

export const FFMPEG_TOOLS_STORAGE_KEY = "mc-ffmpeg-path";

let initStarted = false;

/** Apply the saved ffmpeg tools folder once when the desktop app starts. */
export function initFfmpegToolsFromStorage(): void {
  if (initStarted || !isTauri()) return;
  initStarted = true;

  const stored = readPref(FFMPEG_TOOLS_STORAGE_KEY)?.trim();
  if (!stored) return;

  // Loaded on demand: a static import here would put the whole Tauri bridge on
  // the entry chunk, which every browser visitor downloads and never calls.
  void import("./tauri")
    .then(({ setFfmpegToolsDir }) => setFfmpegToolsDir(stored))
    .catch(() => {
      // Startup is best-effort. Settings shows a detailed status when the user opens it.
    });
}

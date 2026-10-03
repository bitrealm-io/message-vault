import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./tauri-check";

/**
 * Save `text` as a file named `fileName`.
 *
 * In a browser the file goes to the browser's downloads, as any download
 * does. The desktop app's window has no downloads, so the app shows the Save
 * dialog and writes the file where the person chose. The window passes only
 * the name and the text: the path comes from the dialog, never the window.
 *
 * Returns false when the person closed the desktop app's dialog without
 * choosing a place, and true once the file is on its way.
 */
export async function saveTextFile(
  fileName: string,
  text: string,
  mediaType: string,
): Promise<boolean> {
  if (isTauri()) {
    return await invoke<boolean>("save_text_file", { fileName, contents: text });
  }
  const url = URL.createObjectURL(new Blob([text], { type: mediaType }));
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
  return true;
}

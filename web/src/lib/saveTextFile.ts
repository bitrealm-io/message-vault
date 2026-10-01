import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { isTauri } from "./tauri-check";

/**
 * Save `text` as a file named `fileName`.
 *
 * In a browser the file goes to the browser's downloads, as any download
 * does. The desktop app's window has no downloads, so it asks where to save
 * and writes the file there itself.
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
    const extension = fileName.split(".").pop() ?? "";
    const path = await save({
      defaultPath: fileName,
      filters: extension ? [{ name: extension.toUpperCase(), extensions: [extension] }] : [],
    });
    if (!path) return false;
    await invoke("save_text_file", { path, contents: text });
    return true;
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

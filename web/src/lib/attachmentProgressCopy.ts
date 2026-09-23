import type { AttachmentMediaMode } from "./types";

/** Human-readable byte size (`"512 MB"`), shared by every import screen that reports size. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = value >= 10 || unit === 0 ? 0 : 1;
  return `${value.toFixed(digits)} ${units[unit]}`;
}

export function formatAttachmentProgress(input: {
  mode: AttachmentMediaMode;
  done: number;
  total: number;
  bytesDone: number;
  bytesTotal: number;
}): string {
  const verb =
    input.mode === "convert" || input.mode === "compress"
      ? "Converted"
      : input.mode === "skip"
        ? "Skipped"
        : "Copied";
  const counts = `${input.done.toLocaleString()}/${input.total.toLocaleString()}`;
  return `${verb} attachments: ${counts} (${formatBytes(input.bytesDone)} / ${formatBytes(input.bytesTotal)})`;
}

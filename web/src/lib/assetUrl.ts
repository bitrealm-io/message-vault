/** Path used to download an attachment by its content hash. */
export function buildAssetPath(sha256: string, source: string): string {
  const sha = sha256.trim();
  const src = source.trim();
  if (!sha) throw new Error("sha256 is required");
  if (!src) throw new Error("source is required");
  return `/v1/assets/${encodeURIComponent(sha)}?source=${encodeURIComponent(src)}`;
}

/** Path of an attachment's preview, named by the content hash of the original. */
export function buildAssetPreviewPath(sha256: string, source: string): string {
  const sha = sha256.trim();
  const src = source.trim();
  if (!sha) throw new Error("sha256 is required");
  if (!src) throw new Error("source is required");
  return `/v1/assets/${encodeURIComponent(sha)}/preview?source=${encodeURIComponent(src)}`;
}

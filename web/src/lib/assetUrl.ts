/**
 * Path used to download an attachment by its content hash. The hash alone
 * names the attachment: the account stores one file per hash, whatever the
 * source it was imported from.
 */
export function buildAssetPath(sha256: string): string {
  const sha = sha256.trim();
  if (!sha) throw new Error("sha256 is required");
  return `/v1/assets/${encodeURIComponent(sha)}`;
}

/** Path of an attachment's preview, named by the content hash of the original. */
export function buildAssetPreviewPath(sha256: string): string {
  const sha = sha256.trim();
  if (!sha) throw new Error("sha256 is required");
  return `/v1/assets/${encodeURIComponent(sha)}/preview`;
}

import { useEffect, useState } from "react";
import { fetchAssetObjectUrl } from "../lib/serverApi";

/**
 * Load an attachment as a temporary blob URL: the original, or with `preview`
 * its preview. Revokes the URL on unmount or when the id changes.
 *
 * Outside TanStack Query on purpose: the component showing the attachment owns
 * the URL and revokes it, which a cache entry cannot do. One of the two named
 * exceptions in `docs/adr/0002-one-way-to-fetch-data-in-the-web-app.md`.
 */
export function useAssetObjectUrl(
  sha256: string | null | undefined,
  preview = false,
): { url: string | null; error: string | null; loading: boolean } {
  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    const sha = sha256?.trim();
    if (!sha) {
      setUrl(null);
      setError(null);
      setLoading(false);
      return;
    }

    let cancelled = false;
    let objectUrl: string | null = null;
    const ac = new AbortController();
    setLoading(true);
    setError(null);
    setUrl(null);

    fetchAssetObjectUrl(sha, { preview, signal: ac.signal })
      .then((next) => {
        if (cancelled) {
          URL.revokeObjectURL(next);
          return;
        }
        objectUrl = next;
        setUrl(next);
        setLoading(false);
      })
      .catch((e: unknown) => {
        if (cancelled || (e instanceof DOMException && e.name === "AbortError")) return;
        setError(e instanceof Error ? e.message : String(e));
        setLoading(false);
      });

    return () => {
      cancelled = true;
      ac.abort();
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [sha256, preview]);

  return { url, error, loading };
}

import { useEffect, useState } from "react";
import { fetchAssetObjectUrl } from "../lib/serverApi";

/** Load a vault attachment as a temporary blob URL. Revokes the URL on unmount or when the id changes. */
export function useAssetObjectUrl(
  sha256: string | null | undefined,
  source: string | null | undefined,
): { url: string | null; error: string | null; loading: boolean } {
  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    const sha = sha256?.trim();
    const src = source?.trim();
    if (!sha || !src) {
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

    fetchAssetObjectUrl(sha, src, ac.signal)
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
  }, [sha256, source]);

  return { url, error, loading };
}

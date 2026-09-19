import { useEffect, useState } from "react";
import {
  checkVaultHealth,
  HEALTH_SUCCESS_RECHECK_MS,
  HEALTH_URL_DEBOUNCE_MS,
  healthBackoffMs,
  type VaultHealthStatus,
} from "./vaultHealth";

/**
 * Probe vault /health for a server URL with debounce and backoff.
 * Pass `null` to stop (login card). An empty string probes this origin.
 */
export function useVaultHealth(serverUrl: string | null): VaultHealthStatus {
  const [status, setStatus] = useState<VaultHealthStatus>("unknown");

  useEffect(() => {
    if (serverUrl === null) {
      setStatus("unknown");
      return;
    }

    let cancelled = false;
    let debounceTimer: ReturnType<typeof setTimeout> | undefined;
    let loopTimer: ReturnType<typeof setTimeout> | undefined;
    let controller: AbortController | null = null;
    let failureIndex = 0;

    const clearLoop = () => {
      if (loopTimer !== undefined) {
        clearTimeout(loopTimer);
        loopTimer = undefined;
      }
    };

    const schedule = (ms: number, fn: () => void) => {
      clearLoop();
      loopTimer = setTimeout(fn, ms);
    };

    const probe = () => {
      if (cancelled) return;
      controller?.abort();
      controller = new AbortController();
      const signal = controller.signal;
      setStatus("checking");

      void (async () => {
        const ok = await checkVaultHealth(serverUrl, signal);
        if (cancelled || signal.aborted) return;
        if (ok) {
          failureIndex = 0;
          setStatus("ok");
          schedule(HEALTH_SUCCESS_RECHECK_MS, probe);
        } else {
          setStatus("fail");
          const delay = healthBackoffMs(failureIndex);
          failureIndex += 1;
          schedule(delay, probe);
        }
      })();
    };

    setStatus("unknown");
    debounceTimer = setTimeout(() => {
      if (cancelled) return;
      probe();
    }, HEALTH_URL_DEBOUNCE_MS);

    return () => {
      cancelled = true;
      if (debounceTimer !== undefined) clearTimeout(debounceTimer);
      clearLoop();
      controller?.abort();
    };
  }, [serverUrl]);

  return status;
}

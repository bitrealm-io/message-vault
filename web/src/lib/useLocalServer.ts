import { useCallback, useEffect, useRef, useState } from "react";
import { type LocalServerStatus, localServerStatus, startLocalServer } from "./localServer";

/** How often a starting server is asked how it is getting on. */
const POLL_MS = 500;

/** What the app says when it cannot even ask for its server to be started. */
function failed(error: unknown): LocalServerStatus {
  return {
    status: "failed",
    reason: "start_failed",
    message: "Message Crate could not be started.",
    details: error instanceof Error ? error.message : String(error),
  };
}

/**
 * The desktop app's own Message Crate, for the login card.
 *
 * While `active`, the app is asked to make sure its server is running and is
 * then asked how that is going until it settles on ready or failed. `retry`
 * asks again after a failure. Inactive (the browser, or an address the person
 * entered) it reports null and starts nothing.
 */
export function useLocalServer(active: boolean): {
  status: LocalServerStatus | null;
  retry: () => void;
} {
  const [status, setStatus] = useState<LocalServerStatus | null>(null);
  // Each run of `start` takes the next number; an answer for an older run, or
  // one that lands after the card has gone, is dropped.
  const run = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const stop = useCallback(() => {
    run.current += 1;
    if (timer.current !== undefined) clearTimeout(timer.current);
    timer.current = undefined;
  }, []);

  const start = useCallback(() => {
    stop();
    const mine = run.current;
    const settle = (next: LocalServerStatus) => {
      if (run.current !== mine) return;
      setStatus(next);
      if (next.status === "starting") {
        timer.current = setTimeout(() => {
          localServerStatus().then(settle, (error: unknown) => settle(failed(error)));
        }, POLL_MS);
      }
    };
    startLocalServer().then(settle, (error: unknown) => settle(failed(error)));
  }, [stop]);

  useEffect(() => {
    if (!active) {
      setStatus(null);
      return;
    }
    start();
    return stop;
  }, [active, start, stop]);

  return { status, retry: start };
}

import type { UnlistenFn } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import type { DesktopJobName } from "../lib/desktopJob";
import { awaitTauriJob, invokeCancel, onExtractEvents, type TauriJobResult } from "../lib/tauri";
import type { ImportIssueEvent, ImportProgressEvent } from "../lib/types";

export type TauriJobRunCallbacks = {
  onLog?: (line: string) => void;
  onProgress?: (event: ImportProgressEvent) => void;
  onIssue?: (event: ImportIssueEvent) => void;
};

/**
 * Shared start/cancel/log state for long-running desktop jobs
 * (extract, push, pull, and similar).
 *
 * Export and Settings → Convert use `run`, which waits for the job's result
 * and holds the desktop job under `job` while it runs. `start` only collects
 * a log; no screen calls it.
 */
export function useTauriJob(options: {
  job: DesktopJobName;
  onError?: (msg: string) => void;
  onProgress?: (event: ImportProgressEvent) => void;
  onIssue?: (event: ImportIssueEvent) => void;
}): {
  running: boolean;
  /** True after the current job reports that it finished successfully. */
  finished: boolean;
  log: string[];
  start: (invokeFn: () => Promise<void>, startErrorLabel: string) => Promise<void>;
  /**
   * Wait for one desktop job to finish. Stops any listeners from a previous
   * `start` call first. Throws if the job reports an error; the caller handles that.
   */
  run: (invokeFn: () => Promise<void>, callbacks?: TauriJobRunCallbacks) => Promise<TauriJobResult>;
  cancel: () => Promise<void>;
} {
  const job = options.job;
  const onError = options.onError;
  const onProgress = options.onProgress;
  const onIssue = options.onIssue;
  const [running, setRunning] = useState(false);
  const [finished, setFinished] = useState(false);
  const [log, setLog] = useState<string[]>([]);
  const unlistenRef = useRef<UnlistenFn | null>(null);

  const tearDown = useCallback(() => {
    unlistenRef.current?.();
    unlistenRef.current = null;
  }, []);

  useEffect(() => () => tearDown(), [tearDown]);

  const start = useCallback(
    async (invokeFn: () => Promise<void>, startErrorLabel: string) => {
      tearDown();
      setRunning(true);
      setFinished(false);
      setLog([]);

      unlistenRef.current = await onExtractEvents({
        onLog: (line) => {
          setLog((prev) => [...prev, line]);
        },
        onProgress,
        onIssue,
        onFinished: (summary) => {
          setLog((prev) => [...prev, summary]);
          setRunning(false);
          setFinished(true);
          tearDown();
        },
        onError: (err) => {
          setLog((prev) => {
            const next = [...prev, `Error: ${err.detail}`];
            if (err.user_message) next.push(err.user_message);
            return next;
          });
          setRunning(false);
          setFinished(false);
          onError?.(err.user_message ?? err.detail);
          tearDown();
        },
      });

      try {
        await invokeFn();
      } catch (err: unknown) {
        setLog((prev) => [
          ...prev,
          `${startErrorLabel}: ${err instanceof Error ? err.message : String(err)}`,
        ]);
        setRunning(false);
        setFinished(false);
        tearDown();
      }
    },
    [onError, onIssue, onProgress, tearDown],
  );

  const run = useCallback(
    async (
      invokeFn: () => Promise<void>,
      callbacks?: TauriJobRunCallbacks,
    ): Promise<TauriJobResult> => {
      // Stop fire-and-forget listeners so only one subscription is active.
      tearDown();
      setRunning(true);
      setFinished(false);
      try {
        const result = await awaitTauriJob(
          job,
          invokeFn,
          callbacks?.onLog,
          callbacks?.onProgress ?? onProgress,
          callbacks?.onIssue ?? onIssue,
        );
        setFinished(true);
        return result;
      } catch (err: unknown) {
        setFinished(false);
        throw err;
      } finally {
        setRunning(false);
      }
    },
    [job, onIssue, onProgress, tearDown],
  );

  const cancel = useCallback(async () => {
    await invokeCancel();
  }, []);

  return { running, finished, log, start, run, cancel };
}

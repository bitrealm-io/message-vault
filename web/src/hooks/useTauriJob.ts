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
 *
 * `Request` is what the screen asked the job to do, such as a folder and a
 * format. Each job is started with one, and `finished` hands back the request
 * of the job that finished, so a success message names what the job wrote
 * rather than what the form holds now.
 */
export function useTauriJob<Request>(options: {
  job: DesktopJobName;
  onError?: (msg: string) => void;
  onProgress?: (event: ImportProgressEvent) => void;
  onIssue?: (event: ImportIssueEvent) => void;
}): {
  running: boolean;
  /**
   * The request the last job was started with, once that job reports that it
   * finished successfully. `null` while a job runs, after a failure, and
   * before the first job.
   */
  finished: Request | null;
  log: string[];
  start: (
    invokeFn: () => Promise<void>,
    request: Request,
    startErrorLabel: string,
  ) => Promise<void>;
  /**
   * Wait for one desktop job to finish. Stops any listeners from a previous
   * `start` call first. Throws if the job reports an error; the caller handles that.
   */
  run: (
    invokeFn: () => Promise<void>,
    request: Request,
    callbacks?: TauriJobRunCallbacks,
  ) => Promise<TauriJobResult>;
  cancel: () => Promise<void>;
} {
  const job = options.job;
  const onError = options.onError;
  const onProgress = options.onProgress;
  const onIssue = options.onIssue;
  const [running, setRunning] = useState(false);
  const [finished, setFinished] = useState<Request | null>(null);
  const [log, setLog] = useState<string[]>([]);
  const unlistenRef = useRef<UnlistenFn | null>(null);

  const tearDown = useCallback(() => {
    unlistenRef.current?.();
    unlistenRef.current = null;
  }, []);

  useEffect(() => () => tearDown(), [tearDown]);

  const start = useCallback(
    async (invokeFn: () => Promise<void>, request: Request, startErrorLabel: string) => {
      tearDown();
      setRunning(true);
      setFinished(null);
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
          setFinished(() => request);
          tearDown();
        },
        onError: (err) => {
          setLog((prev) => {
            const next = [...prev, `Error: ${err.detail}`];
            if (err.user_message) next.push(err.user_message);
            return next;
          });
          setRunning(false);
          setFinished(null);
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
        setFinished(null);
        tearDown();
      }
    },
    [onError, onIssue, onProgress, tearDown],
  );

  const run = useCallback(
    async (
      invokeFn: () => Promise<void>,
      request: Request,
      callbacks?: TauriJobRunCallbacks,
    ): Promise<TauriJobResult> => {
      // Stop fire-and-forget listeners so only one subscription is active.
      tearDown();
      setRunning(true);
      setFinished(null);
      try {
        const result = await awaitTauriJob(
          job,
          invokeFn,
          callbacks?.onLog,
          callbacks?.onProgress ?? onProgress,
          callbacks?.onIssue ?? onIssue,
        );
        // The updater form, so a request that is itself a function is stored as is.
        setFinished(() => request);
        return result;
      } catch (err: unknown) {
        setFinished(null);
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

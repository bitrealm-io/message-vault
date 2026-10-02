import { useSyncExternalStore } from "react";

/** The screens that run desktop jobs, by the name of what they run. */
export type DesktopJobName = "Import Run" | "Export" | "Convert";

/**
 * Which desktop job is running in this window, if any.
 *
 * The desktop runs one job at a time and refuses a second one
 * (`src-tauri/src/commands/jobs.rs`). `awaitTauriJob` holds this while its
 * job runs, so Import, Export and Settings → Convert can keep their Start
 * buttons off and say which job is running, rather than start a job the
 * desktop refuses.
 */
let current: { name: DesktopJobName } | null = null;
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

/**
 * Mark `name` as the desktop job that is running. Returns the function that
 * marks it ended; it does nothing once another hold has replaced this one.
 */
export function holdDesktopJob(name: DesktopJobName): () => void {
  const hold = { name };
  current = hold;
  notify();
  return () => {
    if (current !== hold) return;
    current = null;
    notify();
  };
}

/** The desktop job that is running, or null. */
export function currentDesktopJob(): DesktopJobName | null {
  return current?.name ?? null;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The desktop job that is running, or null, kept up to date. */
export function useDesktopJob(): DesktopJobName | null {
  return useSyncExternalStore(subscribe, currentDesktopJob, currentDesktopJob);
}

const RUNNING: Record<DesktopJobName, string> = {
  "Import Run": "An Import Run is running.",
  Export: "An export is running.",
  Convert: "A conversion in Settings is running.",
};

/** Why `start` can't start: the job that is running, and when it can. */
export function desktopJobRunningText(running: DesktopJobName, start: string): string {
  return `${RUNNING[running]} ${start} can start once it ends.`;
}

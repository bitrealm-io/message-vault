import type { ReactNode } from "react";
import { type DesktopJobName, desktopJobRunningText, useDesktopJob } from "../lib/desktopJob";
import { isTauri } from "../lib/tauri-check";
import Button from "./Button";
import ProgressBar from "./ProgressBar";

export type TauriJobFormShellProps = {
  /** Screen heading. Omit when the caller already sits under a heading (Settings tabs). */
  title?: string;
  /**
   * The screen's own desktop job. Start stays off while a different one runs,
   * because the desktop runs one job at a time and would refuse this one.
   */
  job: DesktopJobName;
  /** Wrapper classes. Standalone screens keep the default; an embedded tool passes its own. */
  className?: string;
  children: ReactNode;
  startLabel: string;
  runningLabel?: string;
  onStart: () => void;
  onCancel: () => void;
  running: boolean;
  startDisabled?: boolean;
  log: string[];
  error?: string | null;
  success?: ReactNode;
  requireTauri?: boolean;
  intro?: ReactNode;
};

export default function TauriJobFormShell({
  title,
  job,
  className = "max-w-[700px] p-6",
  children,
  startLabel,
  runningLabel,
  onStart,
  onCancel,
  running,
  startDisabled = false,
  log,
  error,
  success,
  requireTauri,
  intro,
}: TauriJobFormShellProps) {
  const otherJob = useDesktopJob();
  if (requireTauri && !isTauri()) {
    return <div className="max-w-[700px] p-6 text-muted">Export requires the desktop app.</div>;
  }

  const blockedBy = !running && otherJob !== null && otherJob !== job ? otherJob : null;
  const disabled = running || startDisabled || blockedBy !== null;

  return (
    <div className={className}>
      {title ? <h2 className="m-0 mb-6">{title}</h2> : null}
      {intro}
      {children}

      {blockedBy ? (
        <p role="status" className="mt-6 mb-0 text-[0.813rem] text-muted">
          {desktopJobRunningText(blockedBy, startLabel)}
        </p>
      ) : null}

      <div className="mt-6 flex gap-3">
        <Button variant="primary" onClick={onStart} disabled={disabled} size="wide">
          {running ? (runningLabel ?? `${startLabel}…`) : startLabel}
        </Button>
        <Button onClick={onCancel} disabled={!running} size="wide">
          Cancel
        </Button>
      </div>

      {error ? (
        <div className="mt-4 rounded border border-danger-soft-border bg-danger-soft-bg p-3 text-[0.813rem] text-danger">
          {error}
        </div>
      ) : null}

      <div className="mt-6">
        <ProgressBar log={log} running={running} />
      </div>

      {success}
    </div>
  );
}

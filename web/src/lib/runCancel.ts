import { invokeCancel } from "./tauri";

/**
 * The error a job refused by a Cancel throws: the same word the desktop side
 * reports for a job it cancelled (`message-crate-core`'s `check_cancel`), so
 * a caller handles both alike.
 */
export const CANCELLED_MESSAGE = "cancelled";

/**
 * The Cancel of one run of desktop jobs: an Import Run, or an export's pull
 * and conversion.
 *
 * The desktop side stops only the job that is running, and each job command
 * starts its job with a cancel flag of its own
 * (`src-tauri/src/commands/jobs.rs`). A Cancel pressed while the run is
 * between jobs, or while a job command is starting, finds no job to stop and
 * would be lost. So the run remembers its Cancel here: `guard` refuses to
 * start a job once the run is cancelled, and sends the Cancel again after a
 * job command returns, when the run was cancelled while that command was
 * starting.
 */
export type RunCancel = {
  /** Record the Cancel for this run and stop the job that is running. */
  cancel: () => Promise<void>;
  /**
   * Wrap the invoke of a job command. The wrapped call throws
   * `CANCELLED_MESSAGE` without invoking when the run is cancelled.
   */
  guard: (invokeFn: () => Promise<void>) => () => Promise<void>;
};

export function createRunCancel(): RunCancel {
  let cancelled = false;
  return {
    cancel: async () => {
      cancelled = true;
      await invokeCancel();
    },
    guard: (invokeFn) => async () => {
      if (cancelled) throw new Error(CANCELLED_MESSAGE);
      await invokeFn();
      if (cancelled) await invokeCancel();
    },
  };
}

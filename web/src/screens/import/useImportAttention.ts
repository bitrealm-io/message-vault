import { getActiveImportSession } from "../../lib/importSession";
import { keys } from "../../lib/queryKeys";
import { useRouteQuery } from "../../lib/routeQuery";
import type { ImportPhase } from "./importProgressState";
import { isReviewPhase, useImportRunState } from "./importRunStore";

/** Why the Import sidebar entry carries a badge, or null when it does not. */
export type ImportAttention = "waiting" | "paused" | "failed";

/** The server's stages at which a run is waiting for the person. */
const WAITING_STAGES = new Set(["staging_review", "media_review"]);

/**
 * The badge for the run this window drives (`phase`, `status`) and the
 * stage the server holds for the account's open run (`serverStage`).
 *
 * The run this window drives wins whenever it has one. Otherwise an open
 * run at a Review is waiting, and one at its Upload is paused: no window is
 * uploading it, so it waits for Resume or Discard. An Upload that fails is
 * paused too (#1233), so the run that would have read as failed reads as
 * paused.
 */
export function importAttentionFor(
  phase: ImportPhase,
  status: string | undefined,
  serverStage: string | null | undefined,
): ImportAttention | null {
  if (isReviewPhase(phase)) return "waiting";
  if (phase === "done" && status === "failed") return "failed";
  if (phase === "done" && status === "paused") return "paused";
  if (phase !== "form" || !serverStage) return null;
  if (WAITING_STAGES.has(serverStage)) return "waiting";
  return serverStage === "upload" ? "paused" : null;
}

/**
 * Whether the account's Import Run needs the person: waiting at a review,
 * paused at its Upload, or finished by failing. Read by the sidebar so a run
 * left on its own is not forgotten on another screen.
 *
 * The run this window drives answers from the store. A run left waiting
 * before the app was closed is only on the server, so the server is asked too,
 * through the same entry the Import screen's resume check fetches each time
 * it shows the form. A Discard or Restart from the resume panel marks that
 * entry stale, so the badge goes with the run.
 */
export function useImportAttention(enabled: boolean): ImportAttention | null {
  const run = useImportRunState();
  const running = useRouteQuery(keys.imports.running, (signal) => getActiveImportSession(signal), {
    enabled,
    staleTime: 30_000,
  });
  return importAttentionFor(run.phase, run.summaryView?.status, running.data?.stage);
}

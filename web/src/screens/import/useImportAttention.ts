import { getActiveImportSession } from "../../lib/importSession";
import { keys } from "../../lib/queryKeys";
import { useRouteQuery } from "../../lib/routeQuery";
import { isReviewPhase, useImportRunState } from "./importRunStore";

/** Why the Import sidebar entry carries a badge, or null when it does not. */
export type ImportAttention = "waiting" | "failed";

/** The server's stages at which a run is waiting for the person. */
const WAITING_STAGES = new Set(["awaiting_gate_1", "awaiting_gate_2"]);

/**
 * Whether the account's Import Run needs the person: waiting at a review,
 * or finished by failing. Read by the sidebar so a run left on its own is
 * not forgotten on another screen.
 *
 * The run this window drives answers from the store. A run left waiting
 * before the app was closed is only on the server, so the server is asked too,
 * through the same entry the Import screen's resume check fetches each time
 * it shows the form. A Discard or Restart from the resume panel marks that
 * entry stale, so the badge goes with the run. The store wins whenever it has
 * a run of its own.
 */
export function useImportAttention(enabled: boolean): ImportAttention | null {
  const run = useImportRunState();
  const running = useRouteQuery(keys.imports.running, (signal) => getActiveImportSession(signal), {
    enabled,
    staleTime: 30_000,
  });
  if (isReviewPhase(run.phase)) return "waiting";
  if (run.phase === "done" && run.summaryView?.status === "failed") return "failed";
  if (run.phase !== "form") return null;
  const stage = running.data?.stage;
  return stage && WAITING_STAGES.has(stage) ? "waiting" : null;
}

import { listImports } from "../../lib/vaultApi";
import { useVaultQuery } from "../../lib/vaultQuery";
import { isApprovalPhase, useImportRunState } from "./importRunStore";

/** Why the Import sidebar entry carries a badge, or null when it does not. */
export type ImportAttention = "waiting" | "failed";

/** The vault's stages at which a run is waiting for the person. */
const WAITING_STAGES = new Set(["awaiting_gate_1", "awaiting_gate_2"]);

/**
 * Whether the account's Import Run needs the person: waiting at an approval,
 * or finished by failing. Read by the sidebar so a run left on its own is
 * not forgotten on another screen.
 *
 * The run this window drives answers from the store. A run left waiting
 * before the app was closed is only on the vault, so the vault is asked too;
 * that query is cheap and stale for a while, and the store wins whenever it
 * has a run of its own.
 */
export function useImportAttention(enabled: boolean): ImportAttention | null {
  const run = useImportRunState();
  const vault = useVaultQuery(
    ["imports", "running"],
    (signal) => listImports({ status: "running", limit: 1 }, { signal }),
    { enabled, staleTime: 30_000 },
  );
  if (isApprovalPhase(run.phase)) return "waiting";
  if (run.phase === "done" && run.summaryView?.status === "failed") return "failed";
  if (run.phase !== "form") return null;
  const stage = vault.data?.items[0]?.stage;
  return stage && WAITING_STAGES.has(stage) ? "waiting" : null;
}

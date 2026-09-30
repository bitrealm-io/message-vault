import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { keys } from "../../lib/queryKeys";
import { useRouteQuery } from "../../lib/routeQuery";
import { getServerStorage } from "../../lib/serverApi";
import { ContentsSection } from "./dashboard/ContentsSection";
import { DatabaseSection } from "./dashboard/DatabaseSection";
import { MessagesByAccountSection } from "./dashboard/MessagesByAccountSection";

/**
 * The Dashboard: a column of headed sections about the whole vault.
 *
 * The owner administers the vault, and administering it starts with knowing
 * how much it holds and where the disk goes. Every section reads the one
 * storage query, so the page shows one loading line and one error line. The
 * figures are counts and bytes and nothing else: no message, contact or
 * conversation is named here, and an account appears only as a username
 * beside numbers (`docs/adr/0008-the-owner-holds-no-messages.md`).
 */
export function OwnerDashboardPanel() {
  const { data, isPending, error } = useRouteQuery(keys.serverStorage.all, (signal) =>
    getServerStorage({ signal }),
  );

  return (
    <section>
      <h3 className="m-0 text-text">Dashboard</h3>
      <p className="mt-[0.35rem] text-[0.875rem] text-muted">
        What this Message Crate holds, across every account.
      </p>

      {isPending ? (
        <p className="mt-4 text-[0.875rem] text-muted">Loading totals…</p>
      ) : error ? (
        <p className="mt-4 text-[0.875rem] text-danger" role="alert">
          {apiErrorMessage(error, "Could not load the totals.")}
        </p>
      ) : (
        <>
          <ContentsSection storage={data} />
          <DatabaseSection storage={data} />
          <MessagesByAccountSection storage={data} />
        </>
      )}
    </section>
  );
}

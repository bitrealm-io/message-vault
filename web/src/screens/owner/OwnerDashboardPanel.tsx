import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { getVaultStorage } from "../../lib/vaultApi";
import { keys } from "../../lib/vaultKeys";
import { useVaultQuery } from "../../lib/vaultQuery";
import { countOf, formatBytes } from "../settings/storage/storageUtils";

/**
 * The Dashboard: what the whole vault holds, summed over every account.
 *
 * The owner administers the vault, and administering it starts with knowing
 * how much it holds. These are counts and a byte total and nothing else: no
 * message, contact or conversation is named here, and the per-account
 * breakdown is each account's Storage tab under User Accounts
 * (`docs/adr/0008-the-vault-owner-holds-no-messages.md`).
 */
export function OwnerDashboardPanel() {
  const { data, isPending, error } = useVaultQuery(keys.vaultStorage.all, (signal) =>
    getVaultStorage({ signal }),
  );

  return (
    <section>
      <h3 className="m-0 text-text">Dashboard</h3>
      <p className="mt-[0.35rem] text-[0.875rem] text-muted">
        What this vault holds, across every account.
      </p>

      {isPending ? (
        <p className="mt-4 text-[0.875rem] text-muted">Loading totals…</p>
      ) : error ? (
        <p className="mt-4 text-[0.875rem] text-danger" role="alert">
          {apiErrorMessage(error, "Could not load the vault's totals.")}
        </p>
      ) : (
        <div className="mt-4 rounded-xl border border-border bg-elevated p-4">
          <div className="text-[1.375rem] font-semibold text-text">
            {formatBytes(data.total_bytes)}
          </div>
          <div className="mt-1 text-[0.813rem] text-muted">
            {countOf(data.message_count, "message")}, {countOf(data.attachment_count, "attachment")}
          </div>
          <div className="mt-0.5 text-[0.813rem] text-muted">
            {countOf(data.conversation_count, "conversation")},{" "}
            {countOf(data.contact_count, "contact")}
          </div>
        </div>
      )}
    </section>
  );
}

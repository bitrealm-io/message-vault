import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { getAccountStorage } from "../../lib/vaultApi";
import { keys } from "../../lib/vaultKeys";
import { useVaultQuery } from "../../lib/vaultQuery";
import StorageUsageCard from "./storage/StorageUsageCard";

/**
 * The Storage tab of an account the vault owner opened from User Accounts:
 * how much the account holds, and nothing of what it holds. The import and
 * export history and the largest attachments name files and people, and the
 * owner reads none of an account's content.
 */
export function ManagedStoragePanel({ accountId }: { accountId: number }) {
  const { data, isPending, error } = useVaultQuery(
    keys.ownerAccounts.storage(accountId),
    (signal) => getAccountStorage({ signal }, accountId),
  );

  if (isPending) return <div className="text-[0.875rem] text-muted">Loading storage…</div>;
  if (error) {
    return (
      <div className="rounded-md border border-danger-soft-border bg-danger-soft-bg p-2 px-3 text-[0.813rem] text-danger">
        {apiErrorMessage(error, "Could not load storage.")}
      </div>
    );
  }

  return (
    <StorageUsageCard
      totalBytes={data?.total_bytes ?? 0}
      attachmentCount={data?.attachment_count ?? 0}
    />
  );
}

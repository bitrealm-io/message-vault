import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { getAccountStorage } from "../../lib/vaultApi";
import { keys } from "../../lib/vaultKeys";
import { useVaultQuery } from "../../lib/vaultQuery";
import StorageUsageCard from "./storage/StorageUsageCard";

/**
 * The Storage tab of an account the vault owner opened from User Accounts:
 * how much the account holds. ADR 0008 lets the owner read more metadata
 * than this (import and export history, attachment file names and sizes);
 * those routes refuse the owner today, so the usage total is what is built.
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

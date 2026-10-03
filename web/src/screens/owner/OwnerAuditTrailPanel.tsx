import { useState } from "react";
import AuditTrailTable from "../auditTrail/AuditTrailTable";
import { useAuditTrail } from "../auditTrail/useAuditTrail";
import { sectionHint } from "../settings/storage/storageUtils";
import { useOwnerAccounts } from "./useOwnerAccounts";

/**
 * Owner Home's Audit Trail: what each user did on this Message Crate, and
 * when, every account's entries and runs in one list, newest first.
 *
 * The account picker narrows the list to one account's entries, the ones
 * its holder reads under Settings. A deleted account is no longer in the
 * picker; its entries stay in the full list under its old username.
 */
export function OwnerAuditTrailPanel() {
  const [accountId, setAccountId] = useState<number | null>(null);
  const { accounts } = useOwnerAccounts();
  const trail = useAuditTrail(
    accountId === null ? { kind: "all" } : { kind: "account", id: accountId },
  );

  return (
    <section>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="m-0 text-text">Audit Trail</h3>
        <label className="flex items-center gap-2 text-[0.813rem] text-muted">
          Account
          <select
            className="rounded border border-border bg-elevated px-2 py-1 text-[0.813rem] text-text"
            value={accountId ?? ""}
            onChange={(event) =>
              setAccountId(event.target.value === "" ? null : Number(event.target.value))
            }
          >
            <option value="">Every account</option>
            {accounts.map((account) => (
              <option key={account.account_id} value={account.account_id}>
                {account.username}
              </option>
            ))}
          </select>
        </label>
      </div>
      <p className={sectionHint}>
        Logins, imports, exports and every change to an account, newest first. Entries are never
        changed or removed, and stay after an account is deleted.
      </p>
      {trail.loading ? (
        <p className="text-[0.875rem] text-muted">Loading the Audit Trail…</p>
      ) : trail.error ? (
        <p className="text-[0.875rem] text-danger">{trail.error}</p>
      ) : (
        <AuditTrailTable
          entries={trail.entries}
          total={trail.total}
          page={trail.page}
          onPageChange={trail.setPage}
          showAccount={accountId === null}
        />
      )}
    </section>
  );
}

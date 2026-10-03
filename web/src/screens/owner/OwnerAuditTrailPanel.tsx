import { useState } from "react";
import Select, { ListBoxItem, selectItemClassName } from "../../components/Select";
import AuditTrail from "../auditTrail/AuditTrail";
import { sectionHint } from "../settings/storage/storageUtils";
import { useOwnerAccounts } from "./useOwnerAccounts";

/** The picker's key for the full list, beside each account's id. */
const EVERY_ACCOUNT = "all";

const itemClassName = (state: { isFocused: boolean; isSelected: boolean }) =>
  selectItemClassName(state, "sm");

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

  return (
    <section>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="m-0 text-text">Audit Trail</h3>
        <div className="flex items-center gap-2 text-[0.813rem] text-muted">
          <span aria-hidden="true">Account</span>
          <Select
            aria-label="Account"
            size="sm"
            className="w-[12rem]"
            selectedKey={accountId === null ? EVERY_ACCOUNT : String(accountId)}
            onSelectionChange={(key) => {
              if (key == null) return;
              setAccountId(key === EVERY_ACCOUNT ? null : Number(key));
            }}
          >
            <ListBoxItem id={EVERY_ACCOUNT} className={itemClassName}>
              Every account
            </ListBoxItem>
            {accounts.map((account) => (
              <ListBoxItem
                key={account.account_id}
                id={String(account.account_id)}
                className={itemClassName}
              >
                {account.username}
              </ListBoxItem>
            ))}
          </Select>
        </div>
      </div>
      <p className={sectionHint}>
        Logins, imports, exports and every change to an account, newest first. Entries are never
        changed or removed, and stay after an account is deleted.
      </p>
      <AuditTrail
        of={accountId === null ? { kind: "all" } : { kind: "account", id: accountId }}
        showAccount={accountId === null}
      />
    </section>
  );
}

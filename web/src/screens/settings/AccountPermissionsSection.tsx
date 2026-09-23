import Checkbox from "../../components/Checkbox";
import Select, { ListBoxItem, selectItemClassName } from "../../components/Select";
import type { AccountProfile } from "../../lib/account";
import { parseSelectKey } from "../../lib/selectKey";
import { type ManagedAccountChanges, useUpdateAccount } from "../owner/useOwnerAccounts";
import { sectionTitleClass } from "./profileStyles";

const STATUSES = ["active", "disabled"] as const;

// Every permission covers messages and their attachments alike, so the
// heading says so once and each row is the bare verb.
const PERMISSIONS = [
  { flag: "can_import", label: "Import" },
  { flag: "can_export", label: "Export" },
  { flag: "can_delete", label: "Delete" },
] as const;

/**
 * An account's status and what it may do with messages.
 *
 * The vault owner sets all four, so they change only with `managedAccountId`,
 * on an account the owner opened from User Accounts. The account holder reads
 * the same section with nothing in it to change.
 */
export function AccountPermissionsSection({
  profile,
  managedAccountId,
}: {
  profile: AccountProfile;
  managedAccountId?: number;
}) {
  const updateAccount = useUpdateAccount();
  const managed = managedAccountId !== undefined;
  // Not locked while a change is sent: the vault answers in a moment, and
  // greying all four controls for that moment reads as a flash.
  const locked = !managed;

  const change = (changes: ManagedAccountChanges) => {
    if (managedAccountId === undefined) return;
    updateAccount.mutate({ id: managedAccountId, changes });
  };

  return (
    <>
      <h3 className={sectionTitleClass}>Status</h3>
      <div className="mb-6">
        {managed ? (
          <Select
            selectedKey={profile.disabled ? "disabled" : "active"}
            isDisabled={locked}
            aria-label="Status"
            className="w-[10rem]"
            onSelectionChange={(key) => {
              const next = parseSelectKey(key, STATUSES);
              if (next) change({ disabled: next === "disabled" });
            }}
          >
            <ListBoxItem id="active" className={selectItemClassName}>
              Active
            </ListBoxItem>
            <ListBoxItem id="disabled" className={selectItemClassName}>
              Disabled
            </ListBoxItem>
          </Select>
        ) : (
          <p className="m-0 text-[0.875rem]">{profile.disabled ? "Disabled" : "Active"}</p>
        )}
      </div>

      <h3 className={sectionTitleClass}>Message Permissions</h3>
      <p className="mb-2 mt-0 text-[0.813rem] text-muted">Messages and their attachments.</p>
      <div className="mb-6 flex flex-col gap-2">
        {PERMISSIONS.map(({ flag, label }) => (
          <Checkbox
            key={flag}
            checked={profile[flag]}
            disabled={locked}
            onChange={(checked) => change({ [flag]: checked })}
            labelClassName="text-[0.875rem]"
          >
            {label}
          </Checkbox>
        ))}
        {managed ? null : (
          <p className="m-0 text-[0.813rem] text-muted">
            The vault owner sets your status and permissions.
          </p>
        )}
        {updateAccount.error ? (
          <p className="m-0 text-[0.813rem] text-danger" role="alert">
            {updateAccount.error.message}
          </p>
        ) : null}
      </div>
    </>
  );
}

import { useNavigate } from "react-router-dom";
import Button from "../../components/Button";
import Checkbox from "../../components/Checkbox";
import Select, { ListBoxItem, selectItemClassName } from "../../components/Select";
import TextField from "../../components/TextField";
import { formatDateTime } from "../../lib/formatDate";
import { parseSelectKey } from "../../lib/selectKey";
import { tdClass, tdMuted, thClass } from "../settings/apiTokensUtils";
import { formatBytes } from "../settings/storage/storageUtils";
import { type ManagedAccount, useOwnerAccounts } from "./useOwnerAccounts";

const STATUSES = ["active", "disabled"] as const;

/** Columns the table has, which the "no match" row spans. */
const COLUMN_COUNT = 8;

/** Shown under the second password field once both are filled and differ. */
function MismatchNote({ first, second }: { first: string; second: string }) {
  if (!first || !second || first === second) return null;
  return (
    <p className="mt-1 text-[0.75rem] text-danger" role="alert">
      Passwords do not match.
    </p>
  );
}

/** A password may be saved once it is typed twice the same way. */
function passwordsAgree(first: string, second: string): boolean {
  return first !== "" && first === second;
}

/** Whether the search bar's words are in the account's username or preferred name. */
function matches(account: ManagedAccount, needle: string): boolean {
  return (
    account.username.toLowerCase().includes(needle) ||
    (account.preferred_name ?? "").toLowerCase().includes(needle)
  );
}

/**
 * The accounts of this vault, the vault owner's own first.
 *
 * A row carries a username, a preferred name, a status, a message count and a
 * storage total — never a message. The name opens the account's Settings,
 * which is where its password is set and where its messages or the account
 * itself are deleted; the table keeps what is set at a glance, status and the
 * three permissions. The owner's row has neither: the owner cannot be
 * disabled and holds no messages to import, export or delete.
 */
export function OwnerAccountsPanel({ filter = "" }: { filter?: string }) {
  const navigate = useNavigate();
  const {
    accounts,
    loading,
    loadError,
    busy,
    actionError,
    clearError,
    patch,
    composing,
    setComposing,
    newUsername,
    setNewUsername,
    newPassword,
    setNewPassword,
    newPasswordConfirm,
    setNewPasswordConfirm,
    cancelCompose,
    createAccount,
  } = useOwnerAccounts();

  if (loading) return <p className="text-[0.875rem] text-muted">Loading accounts…</p>;
  if (loadError) return <p className="text-[0.875rem] text-danger">{loadError}</p>;

  // The header search bar narrows the table by username or preferred name.
  const needle = filter.trim().toLowerCase();
  const shown = needle ? accounts.filter((a) => matches(a, needle)) : accounts;

  return (
    <section>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="m-0 text-text">User Accounts</h3>
        {!composing && (
          <Button
            variant="secondary"
            size="xs"
            disabled={busy}
            onClick={() => {
              clearError();
              setComposing(true);
            }}
          >
            Add account
          </Button>
        )}
      </div>
      <p className="mt-[0.35rem] text-[0.875rem] text-muted">
        The accounts on this vault. Set what each may do here, and open one by its name for its
        settings. You cannot read an account's messages.
      </p>

      {actionError ? (
        <p className="mt-3 text-[0.875rem] text-danger" role="alert">
          {actionError}
        </p>
      ) : null}

      {composing && (
        <div className="mt-3 flex flex-col gap-3 rounded-xl border border-border bg-elevated p-3">
          <div className="flex flex-wrap items-start gap-2">
            <TextField
              value={newUsername}
              onChange={setNewUsername}
              placeholder="Username"
              isDisabled={busy}
              aria-label="New account's username"
              className="min-w-[10rem] flex-1"
            />
            <TextField
              value={newPassword}
              onChange={setNewPassword}
              type="password"
              placeholder="Password"
              isDisabled={busy}
              aria-label="New account's password"
              className="min-w-[10rem] flex-1"
            />
            <div className="min-w-[10rem] flex-1">
              <TextField
                value={newPasswordConfirm}
                onChange={setNewPasswordConfirm}
                type="password"
                placeholder="Confirm password"
                isDisabled={busy}
                aria-label="Confirm the new account's password"
              />
              <MismatchNote first={newPassword} second={newPasswordConfirm} />
            </div>
            <Button
              variant="secondary"
              disabled={
                busy || !newUsername.trim() || !passwordsAgree(newPassword, newPasswordConfirm)
              }
              onClick={() => void createAccount()}
              className="!px-3 !py-1.5 !text-[0.75rem]"
            >
              Save
            </Button>
            <Button
              variant="secondary"
              disabled={busy}
              onClick={cancelCompose}
              className="!px-3 !py-1.5 !text-[0.75rem]"
            >
              Cancel
            </Button>
          </div>
          <p className="text-[0.75rem] text-muted">
            Hand this password over yourself. The person keeps it until they change it under their
            own Settings.
          </p>
        </div>
      )}

      <div className="mt-4 overflow-x-auto rounded-xl border border-border bg-elevated">
        <table className="w-full border-collapse">
          <thead>
            <tr>
              <th className={thClass}>Account</th>
              <th className={thClass}>Status</th>
              <th className={thClass}>Last sign-in</th>
              <th className={thClass}>Messages</th>
              <th className={thClass}>Storage</th>
              <th className={thClass}>Import</th>
              <th className={thClass}>Export</th>
              <th className={thClass}>Delete</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((account) => {
              const preferredName = account.preferred_name?.trim() ?? "";
              return (
                <tr key={account.account_id} className="border-t border-border">
                  <td className={`${tdClass} whitespace-nowrap`}>
                    <button
                      type="button"
                      aria-label={`Settings for ${account.username}`}
                      onClick={() => navigate(`/owner/accounts/${account.account_id}`)}
                      className="cursor-pointer border-none bg-transparent p-0 text-left text-[inherit] font-semibold text-accent hover:underline"
                    >
                      {account.username}
                    </button>
                    {preferredName ? (
                      <div className="text-[0.75rem] text-muted">{preferredName}</div>
                    ) : null}
                  </td>
                  <td className={account.is_owner ? tdMuted : tdClass}>
                    {account.is_owner ? (
                      "Vault owner"
                    ) : (
                      <Select
                        size="sm"
                        selectedKey={account.disabled ? "disabled" : "active"}
                        isDisabled={busy}
                        aria-label={`Status of ${account.username}`}
                        className="w-[7rem]"
                        onSelectionChange={(key) => {
                          const next = parseSelectKey(key, STATUSES);
                          if (next) patch(account.account_id, { disabled: next === "disabled" });
                        }}
                      >
                        <ListBoxItem id="active" className={(s) => selectItemClassName(s, "sm")}>
                          Active
                        </ListBoxItem>
                        <ListBoxItem id="disabled" className={(s) => selectItemClassName(s, "sm")}>
                          Disabled
                        </ListBoxItem>
                      </Select>
                    )}
                  </td>
                  <td className={`${tdMuted} whitespace-nowrap`}>
                    {account.last_sign_in_at ? formatDateTime(account.last_sign_in_at) : "Never"}
                  </td>
                  <td className={tdMuted}>{account.message_count.toLocaleString()}</td>
                  <td className={tdMuted}>{formatBytes(account.storage_bytes)}</td>
                  {account.is_owner ? (
                    <td className={tdMuted} colSpan={3} />
                  ) : (
                    <>
                      <td className={tdClass}>
                        <Checkbox
                          checked={account.can_import}
                          disabled={busy}
                          aria-label={`Allow importing messages for ${account.username}`}
                          onChange={(checked) => patch(account.account_id, { can_import: checked })}
                        />
                      </td>
                      <td className={tdClass}>
                        <Checkbox
                          checked={account.can_export}
                          disabled={busy}
                          aria-label={`Allow exporting messages for ${account.username}`}
                          onChange={(checked) => patch(account.account_id, { can_export: checked })}
                        />
                      </td>
                      <td className={tdClass}>
                        <Checkbox
                          checked={account.can_delete}
                          disabled={busy}
                          aria-label={`Allow deleting messages and attachments for ${account.username}`}
                          onChange={(checked) => patch(account.account_id, { can_delete: checked })}
                        />
                      </td>
                    </>
                  )}
                </tr>
              );
            })}
            {shown.length === 0 && needle ? (
              <tr className="border-t border-border">
                <td className={tdMuted} colSpan={COLUMN_COUNT}>
                  No account matches “{filter.trim()}”.
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </div>
    </section>
  );
}

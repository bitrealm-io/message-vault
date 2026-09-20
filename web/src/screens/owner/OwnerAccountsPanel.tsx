import { useNavigate } from "react-router-dom";
import Button from "../../components/Button";
import { GearIcon } from "../../components/icons";
import NavGlyphButton from "../../components/NavGlyphButton";
import ScrollingTableCard from "../../components/ScrollingTableCard";
import TextField from "../../components/TextField";
import { formatDateTime } from "../../lib/formatDate";
import { tdClass, tdMuted } from "../settings/apiTokensUtils";
import { type ManagedAccount, useOwnerAccounts } from "./useOwnerAccounts";

/** Columns the table has, which the "no match" row spans. */
const COLUMN_COUNT = 4;

/** A column heading: bold, in the text color, so it stands apart from the rows. */
const thClass = "px-3 py-2 text-left text-[0.75rem] font-bold text-text";

/** The line between one column heading and the next. */
const thSeparator = "border-l border-border";

/**
 * Every other row is a shade lighter. The shade is on the cells, and the last
 * row's end cells are rounded, so it follows the card's bottom corners; the
 * card cannot clip it, because a rounded clipping box thins the table's text
 * in the desktop app.
 */
const rowStripe =
  "even:[&>td]:bg-hover/50 last:[&>td:first-child]:rounded-bl-[0.6875rem] last:[&>td:last-child]:rounded-br-[0.6875rem]";

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

/** What the Status column reads for one account. */
function statusLabel(account: ManagedAccount): string {
  if (account.is_owner) return "Vault owner";
  return account.disabled ? "Disabled" : "Active";
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
 * A row carries a username, a preferred name, a status and the last login.
 * The gear at the left of a row, shown while the pointer is in the row, opens
 * the account's Settings, which is where the rest is: the app it connects with
 * under Profile, what it holds under Storage, and its password, status and
 * permissions under Account. The table sets nothing; it shows each status so a
 * disabled account stands out.
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

      <ScrollingTableCard className="mt-4" cardClassName="rounded-xl bg-elevated">
        <table className="w-full border-collapse">
          <thead>
            <tr>
              {/* The gear column has no heading; each gear is labelled with its account. */}
              <td className="w-6 py-2 pl-3" />
              <th className={thClass}>User</th>
              <th className={`${thClass} ${thSeparator}`}>Status</th>
              <th className={`${thClass} ${thSeparator}`}>Last login</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((account) => {
              const preferredName = account.preferred_name?.trim() ?? "";
              return (
                <tr
                  key={account.account_id}
                  className={`group border-t border-border ${rowStripe}`}
                >
                  <td className="w-6 py-2 pl-3 align-middle">
                    <NavGlyphButton
                      aria-label={`Settings for ${account.username}`}
                      onClick={() => navigate(`/owner/accounts/${account.account_id}`)}
                      className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                    >
                      <GearIcon size={15} />
                    </NavGlyphButton>
                  </td>
                  <td className={`${tdClass} whitespace-nowrap`}>
                    <div className="font-semibold">{account.username}</div>
                    {preferredName ? (
                      <div className="text-[0.75rem] text-muted">{preferredName}</div>
                    ) : null}
                  </td>
                  <td className={account.disabled ? tdClass : tdMuted}>{statusLabel(account)}</td>
                  <td className={`${tdMuted} whitespace-nowrap`}>
                    {account.last_login_at ? formatDateTime(account.last_login_at) : "Never"}
                  </td>
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
      </ScrollingTableCard>
    </section>
  );
}

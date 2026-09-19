import { useState } from "react";
import Button from "../../components/Button";
import Checkbox from "../../components/Checkbox";
import ConfirmDialog from "../../components/ConfirmDialog";
import ModalShell, { DialogError, DialogFooter } from "../../components/ModalShell";
import Select, { ListBoxItem, selectItemClassName } from "../../components/Select";
import TextField from "../../components/TextField";
import { formatDateTime } from "../../lib/formatDate";
import { parseSelectKey } from "../../lib/selectKey";
import { tdClass, tdMuted, thClass } from "../settings/apiTokensUtils";
import { formatBytes } from "../settings/storage/storageUtils";
import { type ManagedAccount, useOwnerAccounts } from "./useOwnerAccounts";

type ConfirmTarget = { account: ManagedAccount; kind: "messages" | "account" };

const STATUSES = ["active", "disabled"] as const;

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

function confirmBody(target: ConfirmTarget): string {
  const { account, kind } = target;
  const count = account.message_count.toLocaleString();
  return kind === "messages"
    ? `This permanently deletes ${count} messages belonging to ${account.username}, and their attachments. It cannot be undone.`
    : `This permanently deletes ${account.username}'s account along with ${count} messages and their attachments. It cannot be undone.`;
}

/**
 * The accounts of this vault, and what the vault owner may do to one.
 *
 * A row carries a username, a status, a message count and a storage total —
 * never a message. The counts are what the owner acts on: deleting an
 * account's messages is done on the strength of the number and the account
 * holder's word, not on inspection.
 */
export function OwnerAccountsPanel() {
  const {
    accounts,
    loading,
    loadError,
    busy,
    actionError,
    clearError,
    patch,
    deleteMessages,
    deleteAccount,
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
    passwordTarget,
    resetPassword,
    setResetPassword,
    resetPasswordConfirm,
    setResetPasswordConfirm,
    openPasswordReset,
    closePasswordReset,
    setAccountPassword,
    clearAccountPassword,
  } = useOwnerAccounts();
  const [confirming, setConfirming] = useState<ConfirmTarget | null>(null);

  if (loading) return <p className="text-[0.875rem] text-muted">Loading accounts…</p>;
  if (loadError) return <p className="text-[0.875rem] text-danger">{loadError}</p>;

  const openConfirm = (target: ConfirmTarget) => {
    clearError();
    setConfirming(target);
  };

  const closeConfirm = () => {
    if (busy) return;
    clearError();
    setConfirming(null);
  };

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
        The people with an account on this vault. You can change what they may do, disable them, or
        delete their messages. You cannot read them.
      </p>

      {/* A failure while the delete confirmation or password dialog is open
          shows inside that dialog instead (via its own `error` prop) — showing
          it here too would duplicate it, and the dialog is where the person is
          looking. */}
      {actionError && confirming === null && passwordTarget === null ? (
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
              <th className={thClass}>Actions</th>
            </tr>
          </thead>
          <tbody>
            {accounts.map((account) => (
              <tr key={account.account_id} className="border-t border-border">
                <td className={tdClass}>{account.username}</td>
                <td className={tdClass}>
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
                </td>
                <td className={tdMuted}>
                  {account.last_sign_in_at ? formatDateTime(account.last_sign_in_at) : "Never"}
                </td>
                <td className={tdMuted}>{account.message_count.toLocaleString()}</td>
                <td className={tdMuted}>{formatBytes(account.storage_bytes)}</td>
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
                <td className={tdClass}>
                  <div className="flex flex-wrap items-center gap-1">
                    <Button
                      variant="secondary"
                      size="xs"
                      disabled={busy}
                      onClick={() => openPasswordReset(account)}
                    >
                      Reset password
                    </Button>
                    <Button
                      variant="secondary"
                      size="xs"
                      disabled={busy}
                      onClick={() => openConfirm({ account, kind: "messages" })}
                    >
                      Delete messages
                    </Button>
                    <Button
                      variant="danger"
                      size="xs"
                      disabled={busy}
                      onClick={() => openConfirm({ account, kind: "account" })}
                    >
                      Delete account
                    </Button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <ConfirmDialog
        open={confirming !== null}
        title={confirming?.kind === "messages" ? "Delete messages" : "Delete account"}
        body={confirming ? confirmBody(confirming) : ""}
        confirmLabel="Delete"
        danger
        busy={busy}
        error={actionError}
        onClose={closeConfirm}
        onConfirm={async () => {
          if (!confirming) return;
          const { account, kind } = confirming;
          // These resolve to whether the call actually succeeded: on a refusal
          // the mutation answers `false` and leaves the reason in `actionError`
          // instead of throwing. Close only on success — a 404 must keep the
          // dialog open with the reason showing, not vanish as though the
          // delete had gone through.
          const ok =
            kind === "messages"
              ? await deleteMessages(account.account_id)
              : await deleteAccount(account.account_id);
          if (ok) setConfirming(null);
        }}
      />

      <ModalShell
        open={passwordTarget !== null}
        onOpenChange={(o) => {
          if (!o) closePasswordReset();
        }}
        dismissable={!busy}
        label="Reset password"
        title="Reset password"
        onClose={closePasswordReset}
        closeDisabled={busy}
        maxWidth="24rem"
      >
        <p className="mb-3 text-[0.813rem] text-muted">
          Set a new password for {passwordTarget?.username}, or clear it so they sign in with none.
          They keep it until they change it themselves.
        </p>
        <TextField
          label="New password"
          value={resetPassword}
          onChange={setResetPassword}
          type="password"
          isDisabled={busy}
          autoFocus
          className="mb-3"
        />
        <TextField
          label="Confirm password"
          value={resetPasswordConfirm}
          onChange={setResetPasswordConfirm}
          type="password"
          isDisabled={busy}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void setAccountPassword();
            }
          }}
        />
        <MismatchNote first={resetPassword} second={resetPasswordConfirm} />
        <DialogError message={actionError} />
        <DialogFooter>
          <Button onPress={closePasswordReset} isDisabled={busy}>
            Cancel
          </Button>
          <Button onPress={clearAccountPassword} isDisabled={busy}>
            Clear password
          </Button>
          <Button
            variant="primary"
            onPress={() => void setAccountPassword()}
            isDisabled={busy || !passwordsAgree(resetPassword, resetPasswordConfirm)}
          >
            {busy ? "Saving…" : "Save"}
          </Button>
        </DialogFooter>
      </ModalShell>
    </section>
  );
}

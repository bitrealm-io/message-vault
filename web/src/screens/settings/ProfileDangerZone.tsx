import { useState } from "react";
import { useNavigate } from "react-router-dom";
import Button from "../../components/Button";
import ConfirmDialog from "../../components/ConfirmDialog";
import DeleteAccountDialog from "../../components/DeleteAccountDialog";
import { useAuth } from "../../lib/auth";
import { deleteAccount, deleteAllMessages as deleteAllVaultMessages } from "../../lib/vaultApi";
import { useDeleteAccount, useDeleteAccountMessages } from "../owner/useOwnerAccounts";
import { dangerButtonClass } from "./profileStyles";

const dangerButton = `${dangerButtonClass} !box-border !w-auto !min-w-[10.5rem] !whitespace-nowrap !border-transparent !px-3 !py-2 !text-[0.813rem] !shadow-[inset_0_0_0_1px_var(--danger-soft-border)]`;

/**
 * Delete an account's messages, or the account.
 *
 * For the signed-in account, deleting the account asks for its password and
 * signs out. Given `managedAccountId`, the vault owner is deleting someone
 * else's: no password is asked, because the owner does not know it, and the
 * owner lands back on User Accounts. The owner deletes on the strength of the
 * count and the account holder's word, so the confirmation states the count.
 */
export function ProfileDangerZone({
  isDemo,
  username,
  managedAccountId,
  messageCount = 0,
}: {
  isDemo: boolean;
  username: string;
  managedAccountId?: number;
  messageCount?: number;
}) {
  const { logout } = useAuth();
  const navigate = useNavigate();
  const removeManagedAccount = useDeleteAccount();
  const removeManagedMessages = useDeleteAccountMessages();
  const [dangerZoneOpen, setDangerZoneOpen] = useState(false);
  const [confirmDeleteMessagesOpen, setConfirmDeleteMessagesOpen] = useState(false);
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deletingMessages, setDeletingMessages] = useState(false);
  const [dangerError, setDangerError] = useState("");

  const managed = managedAccountId !== undefined;
  const busy = deleting || deletingMessages;
  // The demo lock is the account's own; the owner may delete the demo account.
  const demoLocked = isDemo && !managed;
  const count = messageCount.toLocaleString();

  const deleteAllMessages = async () => {
    if (demoLocked) return;
    setDeletingMessages(true);
    setDangerError("");
    try {
      if (managed) await removeManagedMessages.mutateAsync(managedAccountId);
      else await deleteAllVaultMessages({ confirm: true });
    } catch (e) {
      setDangerError(e instanceof Error ? e.message : String(e));
    } finally {
      setDeletingMessages(false);
      setConfirmDeleteMessagesOpen(false);
    }
  };

  const performDeleteAccount = async (currentPassword: string) => {
    if (demoLocked) return;
    setDeleting(true);
    setDangerError("");
    try {
      if (managed) {
        await removeManagedAccount.mutateAsync(managedAccountId);
        setDeleteDialogOpen(false);
        navigate("/owner/accounts");
        return;
      }
      await deleteAccount({ confirm: true, current_password: currentPassword });
      setDeleteDialogOpen(false);
      logout();
    } catch (e) {
      setDangerError(e instanceof Error ? e.message : String(e));
    } finally {
      setDeleting(false);
    }
  };

  return (
    <>
      <section className="mt-8 border-t border-border pt-6">
        <button
          type="button"
          aria-expanded={dangerZoneOpen}
          onClick={() => setDangerZoneOpen((open) => !open)}
          className="flex w-full cursor-pointer items-center gap-2 border-none bg-transparent p-0 text-left"
        >
          <span
            className={`inline-block text-[0.75rem] text-danger transition-transform duration-150 ${
              dangerZoneOpen ? "rotate-90" : ""
            }`}
          >
            ▶
          </span>
          <span className="text-[0.75rem] font-semibold uppercase tracking-[0.06em] text-danger">
            Danger zone
          </span>
        </button>
        <p className="ml-5 mt-[0.35rem] text-[0.813rem] text-muted">
          {managed
            ? `Delete ${username}'s messages or permanently remove the account.`
            : "Delete messages or permanently remove your account."}
        </p>

        {dangerZoneOpen && (
          <div className="ml-5 mt-4 rounded-xl border-solid border-danger p-5 [border-width:0.75px]">
            <div className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-5 gap-y-5">
              <div className="min-w-0">
                <p className="m-0 text-[0.813rem] font-bold text-text">
                  Delete all messages and attachments
                </p>
                <p className="m-0 mt-0.5 text-[0.813rem] text-muted">
                  {managed
                    ? "The account, its contacts and its settings remain"
                    : "Your contacts and settings remain"}
                </p>
              </div>
              <div className="justify-self-end p-px">
                <Button
                  variant="danger"
                  disabled={busy || demoLocked}
                  onClick={() => setConfirmDeleteMessagesOpen(true)}
                  className={dangerButton}
                  title={demoLocked ? "Unavailable on the demo account" : undefined}
                >
                  {deletingMessages ? "Deleting…" : "Delete all messages"}
                </Button>
              </div>

              <div className="min-w-0">
                <p className="m-0 text-[0.813rem] font-bold text-text">
                  {managed ? "Delete this account" : "Delete your account"}
                </p>
                <p className="m-0 mt-0.5 text-[0.813rem] text-muted">
                  Permanently delete all contacts, messages, and attachments. This can&apos;t be
                  undone.
                </p>
              </div>
              <div className="justify-self-end p-px">
                <Button
                  variant="danger"
                  disabled={busy || demoLocked}
                  onClick={() => setDeleteDialogOpen(true)}
                  className={dangerButton}
                  title={demoLocked ? "Unavailable on the demo account" : undefined}
                >
                  Delete account
                </Button>
              </div>

              {dangerError && (
                <div className="col-span-2 text-[0.813rem] text-danger" role="alert">
                  {dangerError}
                </div>
              )}
            </div>
          </div>
        )}
      </section>

      {managed ? (
        <ConfirmDialog
          open={deleteDialogOpen}
          title={`Delete ${username}'s account?`}
          body={`This permanently deletes ${username}'s account along with ${count} messages and their attachments. It cannot be undone.`}
          confirmLabel="Delete account"
          danger
          busy={deleting}
          error={dangerError}
          onClose={() => {
            if (!deleting) setDeleteDialogOpen(false);
          }}
          onConfirm={() => void performDeleteAccount("")}
        />
      ) : (
        <DeleteAccountDialog
          open={deleteDialogOpen}
          username={username}
          deleting={deleting}
          onClose={() => {
            if (!deleting) setDeleteDialogOpen(false);
          }}
          onConfirm={(password) => void performDeleteAccount(password)}
        />
      )}

      <ConfirmDialog
        open={confirmDeleteMessagesOpen}
        title={managed ? `Delete ${username}'s messages?` : "Delete all messages?"}
        body={
          managed
            ? `This permanently deletes ${count} messages belonging to ${username}, and their attachments. It cannot be undone.`
            : "Delete all messages and attachments? Your contacts and settings will remain."
        }
        confirmLabel="Delete all messages"
        danger
        busy={deletingMessages}
        onClose={() => setConfirmDeleteMessagesOpen(false)}
        onConfirm={() => void deleteAllMessages()}
      />
    </>
  );
}

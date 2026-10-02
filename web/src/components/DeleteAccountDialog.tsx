import { useEffect, useState } from "react";
import Button from "./Button";
import ModalShell, { DialogError } from "./ModalShell";

/**
 * Confirms an account deleting itself. `hasPassword` is the account's
 * `has_password`: the server checks the current password only when one is
 * set, so the dialog asks for it only then and confirms with none otherwise.
 * `error` is why the last confirm failed; the dialog stays open to retry.
 */
export default function DeleteAccountDialog({
  open,
  username,
  hasPassword,
  deleting = false,
  error = "",
  onClose,
  onConfirm,
}: {
  open: boolean;
  username: string;
  hasPassword: boolean;
  deleting?: boolean;
  error?: string;
  onClose: () => void;
  onConfirm: (currentPassword?: string) => void;
}) {
  const [typedUsername, setTypedUsername] = useState("");
  const [password, setPassword] = useState("");

  useEffect(() => {
    if (open) {
      setTypedUsername("");
      setPassword("");
    }
  }, [open]);

  const expected = username.trim();
  const matches =
    expected.length > 0 && typedUsername === expected && (!hasPassword || password.length > 0);

  return (
    <ModalShell
      open={open}
      onOpenChange={(o) => {
        if (!o && !deleting) onClose();
      }}
      dismissable={!deleting}
      label="Delete your account?"
    >
      <button
        type="button"
        aria-label="Close"
        disabled={deleting}
        onClick={onClose}
        className="absolute top-3 right-3 cursor-pointer border-none bg-transparent text-[1.25rem] leading-none text-muted disabled:cursor-not-allowed"
      >
        ×
      </button>

      <h2 className="mb-2 pr-6 text-[1rem] font-semibold text-text">Delete your account?</h2>

      <p className="mt-3 text-[0.875rem] leading-relaxed text-muted">
        This cannot be undone. Your messages, contacts, group chats, profile, and attachments will
        be permanently deleted.
      </p>

      <label className="mt-5 block">
        <span className="text-[0.875rem] text-text">
          Type your username {expected ? <strong>{expected}</strong> : null} to confirm.
        </span>
        <input
          type="text"
          value={typedUsername}
          onChange={(e) => setTypedUsername(e.target.value)}
          disabled={deleting}
          autoComplete="off"
          spellCheck={false}
          className="mt-2 box-border w-full rounded border border-border bg-elevated px-3 py-2 text-[0.875rem] text-text"
        />
      </label>

      {hasPassword ? (
        <label className="mt-4 block">
          <span className="text-[0.875rem] text-text">Current password</span>
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            disabled={deleting}
            autoComplete="current-password"
            className="mt-2 box-border w-full rounded border border-border bg-elevated px-3 py-2 text-[0.875rem] text-text"
          />
        </label>
      ) : null}

      <DialogError message={error} />

      <div className="mt-5 flex justify-end">
        <Button
          variant="danger"
          disabled={deleting || !matches}
          onClick={() => onConfirm(hasPassword ? password : undefined)}
          className="!px-4 !py-2 !text-[0.813rem]"
        >
          {deleting ? "Deleting…" : "Permanently delete my account"}
        </Button>
      </div>
    </ModalShell>
  );
}

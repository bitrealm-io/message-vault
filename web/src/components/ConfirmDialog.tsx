import Button from "./Button";
import ModalShell, { DialogError, DialogFooter } from "./ModalShell";

export default function ConfirmDialog({
  open,
  title,
  body,
  confirmLabel = "OK",
  cancelLabel = "Cancel",
  busyLabel = "Working…",
  danger = false,
  busy = false,
  error = "",
  onConfirm,
  onClose,
}: {
  open: boolean;
  title: string;
  body: React.ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  /** The confirm button's label while `busy`. */
  busyLabel?: string;
  danger?: boolean;
  busy?: boolean;
  /** Why the last confirm failed. The dialog stays open so it can be retried. */
  error?: string;
  onConfirm: () => void;
  onClose: () => void;
}) {
  return (
    <ModalShell
      open={open}
      onOpenChange={(o) => {
        if (!o && !busy) onClose();
      }}
      dismissable={!busy}
      label={title}
      title={title}
      onClose={onClose}
      closeDisabled={busy}
      maxWidth="24rem"
    >
      {typeof body === "string" ? (
        <p className="mt-3 text-[0.875rem] leading-relaxed text-muted">{body}</p>
      ) : (
        body
      )}
      <DialogError message={error} />
      <DialogFooter>
        <Button onPress={onClose} isDisabled={busy}>
          {cancelLabel}
        </Button>
        <Button variant={danger ? "danger" : "primary"} onPress={onConfirm} isDisabled={busy}>
          {busy ? busyLabel : confirmLabel}
        </Button>
      </DialogFooter>
    </ModalShell>
  );
}

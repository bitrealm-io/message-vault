import { useState } from "react";
import Button from "./Button";
import ModalShell from "./ModalShell";

/** Small name form used to create or rename a contact group. */
export default function GroupNameDialog({
  title,
  placeholder = "Name",
  confirmLabel = "Save",
  initial = "",
  error = null,
  busy = false,
  onSave,
  onCancel,
}: {
  title: string;
  placeholder?: string;
  confirmLabel?: string;
  initial?: string;
  error?: string | null;
  busy?: boolean;
  onSave: (name: string) => void | Promise<void>;
  onCancel: () => void;
}) {
  const [name, setName] = useState(initial);

  const submit = () => {
    const trimmed = name.trim();
    if (!trimmed || busy) return;
    void onSave(trimmed);
  };

  return (
    <ModalShell
      open
      onOpenChange={(o) => {
        // While the save is in flight the dialog stays, so its answer (a
        // navigation, or an error to show) lands on a dialog that is still open.
        if (!o && !busy) onCancel();
      }}
      dismissable={!busy}
      label={title}
      maxWidth="22rem"
    >
      <h3 className="mb-3 text-[1rem] text-text">{title}</h3>
      <input
        type="text"
        value={name}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            submit();
          }
        }}
        placeholder={placeholder}
        disabled={busy}
        className="mb-2 box-border w-full rounded border border-border bg-elevated px-2 py-1.5 text-[0.875rem] text-text"
      />
      {error ? (
        <p className="mb-3 text-[0.813rem] text-danger">{error}</p>
      ) : (
        <div className="mb-3" />
      )}
      <div className="flex justify-end gap-2">
        <Button onClick={onCancel} disabled={busy} size="sm">
          Cancel
        </Button>
        <Button
          variant="primary"
          onClick={submit}
          disabled={busy || !name.trim()}
          size="sm"
          className="!px-4"
        >
          {confirmLabel}
        </Button>
      </div>
    </ModalShell>
  );
}

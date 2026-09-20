import { useEffect } from "react";
import Button from "../../components/Button";
import Checkbox from "../../components/Checkbox";
import ModalShell, { DialogFooter } from "../../components/ModalShell";
import TextField from "../../components/TextField";

function PermissionCheckbox({
  checked,
  onChange,
  disabled,
  allowed,
  children,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled: boolean;
  /** Whether the logged-in account itself holds this permission. */
  allowed: boolean;
  children: string;
}) {
  // An admin can now switch an account's own permissions off, so `checked`
  // can arrive `true` for a permission the account no longer holds. Force it
  // unchecked — both the display and the value the form would submit —
  // rather than showing it checked-but-disabled, which reads as a token
  // claiming a right the account does not have.
  useEffect(() => {
    if (!allowed && checked) onChange(false);
  }, [allowed, checked, onChange]);

  return (
    <div>
      <Checkbox checked={checked && allowed} onChange={onChange} disabled={disabled || !allowed}>
        {children}
      </Checkbox>
      {!allowed && (
        <p className="mt-0.5 pl-6 text-[0.688rem] text-muted">Your account cannot do this.</p>
      )}
    </div>
  );
}

export function ApiTokenCreateForm({
  label,
  busy,
  onLabelChange,
  canImport,
  onCanImportChange,
  canExport,
  onCanExportChange,
  canDelete,
  onCanDeleteChange,
  accountCanImport,
  accountCanExport,
  accountCanDelete,
  onSave,
  onCancel,
}: {
  label: string;
  busy: boolean;
  onLabelChange: (value: string) => void;
  canImport: boolean;
  onCanImportChange: (value: boolean) => void;
  canExport: boolean;
  onCanExportChange: (value: boolean) => void;
  canDelete: boolean;
  onCanDeleteChange: (value: boolean) => void;
  /** The logged-in account's own permissions — a token can never exceed them. */
  accountCanImport: boolean;
  accountCanExport: boolean;
  accountCanDelete: boolean;
  onSave: () => void;
  onCancel: () => void;
}) {
  return (
    <div className="mb-3 flex flex-col gap-3 rounded-xl border border-border bg-elevated p-3">
      <div className="flex flex-wrap items-end gap-2">
        <TextField
          value={label}
          onChange={onLabelChange}
          placeholder="Enter API key name…"
          isDisabled={busy}
          aria-label="API key name"
          className="min-w-[12rem] flex-1"
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              onSave();
            }
            if (e.key === "Escape") {
              e.preventDefault();
              onCancel();
            }
          }}
        />
        <Button
          variant="secondary"
          disabled={busy || !label.trim()}
          onClick={onSave}
          className="!px-3 !py-1.5 !text-[0.75rem]"
        >
          Save
        </Button>
        <Button
          variant="secondary"
          disabled={busy}
          onClick={onCancel}
          className="!px-3 !py-1.5 !text-[0.75rem]"
        >
          Cancel
        </Button>
      </div>
      <div className="flex flex-wrap gap-4">
        <PermissionCheckbox
          checked={canImport}
          onChange={onCanImportChange}
          disabled={busy}
          allowed={accountCanImport}
        >
          Import
        </PermissionCheckbox>
        <PermissionCheckbox
          checked={canExport}
          onChange={onCanExportChange}
          disabled={busy}
          allowed={accountCanExport}
        >
          Export
        </PermissionCheckbox>
        <PermissionCheckbox
          checked={canDelete}
          onChange={onCanDeleteChange}
          disabled={busy}
          allowed={accountCanDelete}
        >
          Delete messages and attachments
        </PermissionCheckbox>
      </div>
    </div>
  );
}

export function ApiTokenRenameDialog({
  open,
  busy,
  renameLabel,
  onRenameLabelChange,
  onClose,
  onSave,
}: {
  open: boolean;
  busy: boolean;
  renameLabel: string;
  onRenameLabelChange: (value: string) => void;
  onClose: () => void;
  onSave: () => void;
}) {
  return (
    <ModalShell
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
      dismissable={!busy}
      label="Rename API key"
      title="Rename API key"
      onClose={onClose}
      closeDisabled={busy}
      maxWidth="24rem"
    >
      <p className="mb-3 text-[0.813rem] text-muted">
        Choose a name you will recognize later. The secret value does not change.
      </p>
      <TextField
        label="Name"
        value={renameLabel}
        onChange={onRenameLabelChange}
        isDisabled={busy}
        autoFocus
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            onSave();
          }
        }}
      />
      <DialogFooter>
        <Button onPress={onClose} isDisabled={busy}>
          Cancel
        </Button>
        <Button variant="primary" onPress={onSave} isDisabled={busy || !renameLabel.trim()}>
          {busy ? "Saving…" : "Save"}
        </Button>
      </DialogFooter>
    </ModalShell>
  );
}

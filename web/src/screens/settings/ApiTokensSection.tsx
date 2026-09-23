import ApiTokenRevealDialog from "../../components/ApiTokenRevealDialog";
import Button from "../../components/Button";
import ConfirmDialog from "../../components/ConfirmDialog";
import { ApiTokenCreateForm, ApiTokenRenameDialog } from "./ApiTokenForms";
import ApiTokensTable from "./ApiTokensTable";
import { useApiTokens } from "./useApiTokens";

/** Named API keys for programs (import/export). Separate from the rotating GUI session token. */
export function ApiTokensSection({
  accountCanImport,
  accountCanExport,
}: {
  /** The logged-in account's own permissions — a token can never exceed them. */
  accountCanImport: boolean;
  accountCanExport: boolean;
}) {
  const {
    items,
    loadError,
    busy,
    composing,
    setComposing,
    label,
    setLabel,
    canImport,
    setCanImport,
    canExport,
    setCanExport,
    actionError,
    reveal,
    setReveal,
    revokeTarget,
    setRevokeTarget,
    renameTarget,
    renameLabel,
    setRenameLabel,
    cancelCompose,
    openRename,
    closeRename,
    create,
    rename,
    revoke,
  } = useApiTokens();

  return (
    <div className="mb-6">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <h3 className="mb-0 text-[0.75rem] font-bold text-text">API keys</h3>
        {!composing && (
          <Button
            variant="secondary"
            disabled={busy}
            onClick={() => setComposing(true)}
            className="!rounded-md !border-transparent !bg-text !px-3 !py-1 !text-[0.75rem] !font-semibold !text-bg hover:!brightness-90"
          >
            Add
          </Button>
        )}
      </div>

      {loadError && <div className="mb-3 text-[0.75rem] text-danger">{loadError}</div>}
      {actionError && (
        <div className="mb-3 text-[0.75rem] text-danger" role="alert">
          {actionError}
        </div>
      )}

      {composing && (
        <ApiTokenCreateForm
          label={label}
          busy={busy}
          onLabelChange={setLabel}
          canImport={canImport}
          onCanImportChange={setCanImport}
          canExport={canExport}
          onCanExportChange={setCanExport}
          accountCanImport={accountCanImport}
          accountCanExport={accountCanExport}
          onSave={() => void create()}
          onCancel={cancelCompose}
        />
      )}

      <ApiTokensTable
        items={items}
        busy={busy}
        composing={composing}
        onRename={openRename}
        onRevoke={setRevokeTarget}
      />

      <p className="mt-3 text-[0.75rem] leading-relaxed text-muted">
        API keys give secure, programmatic access so vault tools can import, export, and (if
        granted) delete message data. Treat them like passwords: keep them private and never share
        them publicly.
      </p>

      <ApiTokenRevealDialog
        open={reveal !== null}
        label={reveal?.label ?? ""}
        token={reveal?.token ?? ""}
        onClose={() => setReveal(null)}
      />

      <ApiTokenRenameDialog
        open={renameTarget !== null}
        busy={busy}
        renameLabel={renameLabel}
        onRenameLabelChange={setRenameLabel}
        onClose={closeRename}
        onSave={() => void rename()}
      />

      <ConfirmDialog
        open={revokeTarget !== null}
        title="Delete API key?"
        body={
          revokeTarget
            ? `Delete API key “${revokeTarget.label}”? CLI tools using it will stop working.`
            : ""
        }
        confirmLabel="Delete key"
        danger
        busy={busy}
        onClose={() => setRevokeTarget(null)}
        onConfirm={() => revokeTarget && void revoke(revokeTarget)}
      />
    </div>
  );
}

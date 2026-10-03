import ConfirmDialog from "../../components/ConfirmDialog";
import ApiTokensTable from "./ApiTokensTable";
import { useManagedApiTokens } from "./useApiTokens";

/**
 * The API Tokens of an account the owner opened from User Accounts. The owner
 * sees each token's name, permissions and use, and revokes one that has
 * leaked. Making and renaming a token stay with the account's holder, and the
 * owner never sees any part of a token's secret.
 */
export function ManagedApiTokensSection({ accountId }: { accountId: number }) {
  const { items, loadError, busy, actionError, revokeTarget, setRevokeTarget, revoke } =
    useManagedApiTokens(accountId);

  return (
    <div className="mb-6">
      <h3 className="mb-3 text-[0.75rem] font-bold text-text">API Tokens</h3>

      {loadError && <div className="mb-3 text-[0.75rem] text-danger">{loadError}</div>}
      {actionError && (
        <div className="mb-3 text-[0.75rem] text-danger" role="alert">
          {actionError}
        </div>
      )}

      <ApiTokensTable items={items} busy={busy} composing={false} onRevoke={setRevokeTarget} />

      <p className="mt-3 text-[0.75rem] leading-relaxed text-muted">
        The account's programs use these API Tokens to import and export its messages. Revoking a
        token ends its access at once. Only the account can add or rename one.
      </p>

      <ConfirmDialog
        open={revokeTarget !== null}
        title="Revoke API Token?"
        body={
          revokeTarget
            ? `Revoke API Token “${revokeTarget.label}”? Programs using it will stop working.`
            : ""
        }
        confirmLabel="Revoke token"
        danger
        busy={busy}
        onClose={() => setRevokeTarget(null)}
        onConfirm={() => revokeTarget && void revoke(revokeTarget)}
      />
    </div>
  );
}

import { useMutation } from "@tanstack/react-query";
import Checkbox from "../../components/Checkbox";
import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { useVaultInfo } from "../../lib/useVaultInfo";
import { getVaultSettings, updateVaultSettings } from "../../lib/vaultApi";
import { keys } from "../../lib/vaultKeys";
import { useVaultCache, useVaultQuery } from "../../lib/vaultQuery";

/**
 * Settings that belong to the whole vault rather than to one account.
 *
 * One so far. Public registration is off on a fresh vault, so a vault admits
 * nobody its owner has not admitted until the owner decides otherwise. Under
 * it the vault states which code it runs and which schema its database
 * carries: the Build, and the Schema Fingerprint as the number the vault
 * stamps into the database and names in its startup warning.
 */
export function VaultSettingsPanel() {
  const cache = useVaultCache();
  const { data, isPending, error } = useVaultQuery(keys.vaultSettings.all, (signal) =>
    getVaultSettings({ signal }),
  );
  const save = useMutation({
    mutationFn: (public_registration: boolean) => updateVaultSettings({ public_registration }),
    onSuccess: (settings) => cache.set(keys.vaultSettings.all, settings),
  });
  const info = useVaultInfo();

  if (isPending) return <p className="text-[0.875rem] text-muted">Loading settings…</p>;
  if (error) {
    return (
      <p className="text-[0.875rem] text-danger">
        {apiErrorMessage(error, "Could not load vault settings.")}
      </p>
    );
  }

  return (
    <section>
      <h3 className="m-0 text-text">Settings</h3>
      <p className="mt-[0.35rem] text-[0.875rem] text-muted">
        How this vault behaves, whoever is logged in.
      </p>

      <div className="mt-4 rounded-xl border border-border bg-elevated p-4">
        <Checkbox
          checked={data?.public_registration === true}
          disabled={save.isPending}
          onChange={(checked) => save.mutate(checked)}
        >
          Let anyone reaching this vault create their own account
        </Checkbox>
        <p className="mt-2 text-[0.75rem] text-muted">
          Off: you create every account yourself, and the login screen offers only Login. On: the
          login screen also offers Create Account.
        </p>
        {save.error ? (
          <p className="mt-2 text-[0.813rem] text-danger" role="alert">
            {apiErrorMessage(save.error, "Could not save.")}
          </p>
        ) : null}
      </div>

      {info.data ? (
        <dl className="mt-4 grid grid-cols-[max-content_minmax(0,1fr)] items-baseline gap-x-6 gap-y-2 rounded-xl border border-border bg-elevated p-4 text-[0.875rem]">
          <dt className="text-muted">Version</dt>
          <dd className="m-0 font-mono text-[0.813rem] text-text">{info.data.version}</dd>
          <dt className="text-muted">Schema fingerprint</dt>
          <dd className="m-0 font-mono text-[0.813rem] text-text">
            {info.data.schema_fingerprint}
          </dd>
        </dl>
      ) : null}
    </section>
  );
}

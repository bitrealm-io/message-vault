import { useMutation } from "@tanstack/react-query";
import Checkbox from "../../components/Checkbox";
import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { getVaultSettings, updateVaultSettings } from "../../lib/vaultApi";
import { keys } from "../../lib/vaultKeys";
import { useVaultCache, useVaultQuery } from "../../lib/vaultQuery";

/**
 * Settings that belong to the whole vault rather than to one account.
 *
 * One so far. Public registration is off on a fresh vault, so a vault admits
 * nobody its owner has not admitted until the owner decides otherwise.
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
      <h3 className="m-0 text-text">Vault Settings</h3>
      <p className="mt-[0.35rem] text-[0.875rem] text-muted">
        How this vault behaves, whoever is signed in.
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
          Off: you create every account yourself, and the sign-in screen offers only Login. On: the
          sign-in screen also offers Create Account.
        </p>
        {save.error ? (
          <p className="mt-2 text-[0.813rem] text-danger" role="alert">
            {apiErrorMessage(save.error, "Could not save.")}
          </p>
        ) : null}
      </div>
    </section>
  );
}

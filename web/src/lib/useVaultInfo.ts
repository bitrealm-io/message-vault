import { getVaultState } from "./vaultApi";
import { keys } from "./vaultKeys";
import { useVaultQuery } from "./vaultQuery";

/**
 * What the vault says about itself to a logged-in screen: its Build and its
 * Schema Fingerprint. The same `GET /v1/vault` the entry screen reads for the
 * vault's state (`useVaultState`), asked again here because that query is
 * keyed by address and never goes stale, and a vault's Build changes under an
 * open tab when the vault is upgraded.
 */
export function useVaultInfo() {
  return useVaultQuery(keys.vaultInfo.all, (signal) => getVaultState({ signal }));
}

import { useQuery } from "@tanstack/react-query";
import { setBaseUrl } from "./api";
import { getVaultState } from "./vaultApi";

/** What the vault reports about itself, plus the shapes a screen must handle. */
export type VaultState = "unclaimed" | "closed" | "open";

/**
 * What the entry screen should offer, asked of the vault rather than worked
 * out here.
 *
 * The vault answers with one value — `unclaimed`, `closed`, or `open` — so the
 * rule joining "does an owner exist" to "is registration open" is stated once,
 * on the server. Deriving it again in the browser, and a third time in the
 * desktop app, would be three copies free to drift apart. See
 * `docs/adr/0008-the-vault-owner-holds-no-messages.md`.
 *
 * This is the one query that runs before anyone logs in, so it is a plain
 * `useQuery` rather than `useVaultQuery`: there is no account to name the
 * cache entry with, and the answer belongs to the address, not to a person.
 * `serverUrl` is null while no address has been resolved, which keeps the
 * query idle rather than firing at nothing.
 */
export function useVaultState(serverUrl: string | null): {
  state: VaultState | null;
  loading: boolean;
  error: string;
} {
  const { data, isPending, error } = useQuery({
    queryKey: ["vault-state", serverUrl ?? ""],
    enabled: serverUrl !== null,
    queryFn: async ({ signal }) => {
      if (serverUrl) setBaseUrl(serverUrl);
      const res = await getVaultState({ signal });
      return res.state as VaultState;
    },
    // A vault does not change state under a logged-out visitor except by their
    // own act, and every act that changes it navigates away from this screen.
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });

  return {
    state: data ?? null,
    loading: serverUrl !== null && isPending,
    error: error ? error.message : "",
  };
}

import { useAccountProfile } from "./useAccountProfile";

/**
 * Whether this account still owes profile setup.
 *
 * A server fact, read from the profile, not inferred here from a profile that
 * looks empty and then cached in `localStorage`. The vault decides once and
 * every client gets the same answer, so clearing site data or signing in from
 * a second browser cannot change what the product believes about an account.
 *
 * `loading` matters to the caller for the same reason it does for
 * {@link import("./useMustChangePassword").useMustChangePassword}: a guard that
 * read "not loaded yet" as "nothing owed" would let the account into the app
 * for one render and then pull it back out.
 */
export function useNeedsProfileSetup(): { needsSetup: boolean; loading: boolean } {
  const { profile, loading } = useAccountProfile();
  return { needsSetup: profile?.must_set_up_profile === true, loading };
}

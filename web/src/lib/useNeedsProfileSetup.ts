import { useAccountProfile } from "./useAccountProfile";

/**
 * Whether this account still owes profile setup.
 *
 * A server fact, read from the profile, not inferred here from a profile that
 * looks empty and then cached in `localStorage`. The server decides once and
 * every client gets the same answer, so clearing site data or logging in from
 * a second browser cannot change what the product believes about an account.
 *
 * `loading` matters to the caller: a guard that read "not loaded yet" as
 * "nothing owed" would let the account into the app for one render and then
 * pull it back out. `error` matters for the same reason: a profile that failed
 * to load does not say that nothing is owed.
 */
export function useNeedsProfileSetup(): {
  needsSetup: boolean;
  loading: boolean;
  error: string;
} {
  const { profile, loading, error } = useAccountProfile();
  return { needsSetup: profile?.must_set_up_profile === true, loading, error };
}

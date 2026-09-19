import { useAccountProfile } from "./useAccountProfile";

/**
 * Whether the logged-in principal is this vault's owner.
 *
 * A server fact, read from the profile. The owner has no vault of their own,
 * so every screen built around conversations is meaningless to them and the
 * routing has to know it. `loading` matters to the caller: a guard that
 * treated "not loaded yet" as "an ordinary account" would flash the message
 * shell at someone who has no messages.
 */
export function useIsVaultOwner(): { isOwner: boolean; loading: boolean } {
  const { profile, loading } = useAccountProfile();
  return { isOwner: profile?.is_owner === true, loading };
}

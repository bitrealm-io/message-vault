import { useAccountProfile } from "./useAccountProfile";

/**
 * Whether the logged-in principal is this Message Crate's owner.
 *
 * A server fact, read from the profile. The owner has no messages of their own,
 * so every screen built around conversations is meaningless to them and the
 * routing has to know it. `loading` matters to the caller: a guard that
 * treated "not loaded yet" as "an ordinary account" would flash the message
 * shell at someone who has no messages. `error` matters for the same reason:
 * a profile that failed to load says nothing about who this is.
 */
export function useIsOwner(): { isOwner: boolean; loading: boolean; error: string } {
  const { profile, loading, error } = useAccountProfile();
  return { isOwner: profile?.is_owner === true, loading, error };
}

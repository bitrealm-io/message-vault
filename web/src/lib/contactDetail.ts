import { type UseMutationResult, useMutation } from "@tanstack/react-query";
import { getContact, updateContact } from "./vaultApi";
import type { components } from "./vaultApi.types";
import { keys } from "./vaultKeys";
import { useVaultCache, useVaultQuery } from "./vaultQuery";

/**
 * One contact in full, as the contact drawer shows it.
 *
 * `useContactDetailCache` was the last hand-built piece of it: a `Map`, its
 * own in-flight guard, and a `mv-contact-detail-changed` browser event that
 * the drawer subscribed to so that group chips edited in the contact list
 * would show. All three are TanStack Query's now: the cache is keyed by
 * account and contact, and the group chips a contact-list edit writes are the
 * membership mutation's optimistic patch onto this same entry.
 */

export type ContactDetail = components["schemas"]["Contact"];
export type ContactHandle = components["schemas"]["Identity"];
/** One change to a contact: its name, or one identity added, updated or removed. */
export type ContactChange = components["schemas"]["UpdateContactRequest"];

/** The contact behind an open drawer. Skipped entirely when no contact is open. */
export function useContactDetail(contactId: string | null): {
  detail: ContactDetail | null;
  loading: boolean;
} {
  const { data, isPending } = useVaultQuery(
    keys.contacts.detail(contactId ?? ""),
    (signal) => getContact(contactId ?? "", { signal }),
    { enabled: contactId !== null },
  );
  return { detail: contactId ? (data ?? null) : null, loading: isPending };
}

/**
 * Change one thing about a contact.
 *
 * The vault answers with the contact as it now stands, so the answer goes
 * straight into the entry the drawer reads and nothing asks for it again. The
 * list pages are marked stale because they show the name too; the contact's
 * own entry is not, because it is already right.
 */
export function useUpdateContact(): UseMutationResult<
  ContactDetail,
  Error,
  { contactId: string; body: ContactChange }
> {
  const cache = useVaultCache();
  return useMutation<ContactDetail, Error, { contactId: string; body: ContactChange }>({
    mutationFn: ({ contactId, body }) => updateContact(contactId, body),
    onSuccess: (detail, { contactId }) => {
      cache.set(keys.contacts.detail(contactId), detail);
    },
    onSettled: () => cache.invalidate(keys.contacts.lists),
  });
}

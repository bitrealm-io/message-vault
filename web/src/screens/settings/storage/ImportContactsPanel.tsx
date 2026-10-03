import { type UIEvent, useCallback } from "react";
import { apiErrorMessage } from "../../../lib/apiErrorMessage";
import { keys } from "../../../lib/queryKeys";
import { type PagedFetchPage, useRoutePagedList } from "../../../lib/routeQuery";
import { getImportContacts } from "../../../lib/serverApi";
import type { components } from "../../../lib/serverApi.types";

/** What the run did to one contact, as the server recorded it. */
type ContactReason = components["schemas"]["ContactReason"];

/** One contact an import run created or changed, and why it is listed. */
type ImportContact = components["schemas"]["ImportContact"];

/** The reason as the person reads it. */
const REASON_LABEL: Record<ContactReason, string> = {
  created: "New",
  replaced_trashed: "New, replaces a trashed contact",
  named: "Named",
  identity_added: "Identity added",
};

/** How close to the end of the list, in pixels, scrolling asks for the next page. */
const NEAR_END_PX = 48;

/** A contact the run learned an address for but no name yet. */
const UNNAMED = "(unknown)";

/**
 * The contacts one import run created or changed, each with what the run did
 * to it.
 *
 * A run creates a contact for every participant it meets, so this is where a
 * person sees who arrived with a given backup, and which of them came back
 * from the Trash. Contacts with no name yet are the ones waiting in the
 * Unknown group.
 */
export default function ImportContactsPanel({
  importId,
  newCount,
  changedCount,
}: {
  importId: number;
  newCount: number;
  changedCount: number;
}) {
  const fetchPage = useCallback<PagedFetchPage<ImportContact>>(
    ({ limit, offset, signal }) => getImportContacts(importId, { limit, offset }, { signal }),
    [importId],
  );
  const { items, total, loading, filling, error, loadMore } = useRoutePagedList(
    keys.imports.contacts(importId),
    fetchPage,
  );

  /** Ask for the next page once the person scrolls near the end of the rows loaded. */
  const onScroll = (e: UIEvent<HTMLUListElement>) => {
    const el = e.currentTarget;
    if (el.scrollHeight - el.scrollTop - el.clientHeight <= NEAR_END_PX) loadMore();
  };

  if (loading) return <div className="text-[0.813rem] text-muted">Loading contacts…</div>;
  if (error && items.length === 0) {
    return (
      <div className="text-[0.813rem] text-danger">
        {apiErrorMessage(error, "Could not load contacts for this import.")}
      </div>
    );
  }
  if (items.length === 0) {
    return <div className="text-[0.813rem] text-muted">This import changed no contacts.</div>;
  }

  return (
    <div>
      <p className="mb-2 text-[0.813rem] text-muted">
        {newCount.toLocaleString()} new, {changedCount.toLocaleString()} changed
      </p>
      <ul className="max-h-48 overflow-y-auto text-[0.813rem]" onScroll={onScroll}>
        {items.map((c) => (
          <li key={c.id} className="flex items-center justify-between gap-3 py-0.5">
            <span className={c.name.trim() ? "truncate" : "truncate text-muted"}>
              {c.name.trim() || UNNAMED}
            </span>
            <span className="shrink-0 text-muted">{REASON_LABEL[c.reason]}</span>
          </li>
        ))}
      </ul>
      {filling ? <p className="mt-1 text-[0.813rem] text-muted">Loading more contacts…</p> : null}
      {error ? (
        <p className="mt-1 text-[0.813rem] text-danger">
          {apiErrorMessage(error, "Could not load more contacts for this import.")}
        </p>
      ) : null}
      {items.length < total && !filling && !error ? (
        <p className="mt-1 text-[0.813rem] text-muted">
          {items.length.toLocaleString()} of {total.toLocaleString()} listed. Scroll for more.
        </p>
      ) : null}
    </div>
  );
}

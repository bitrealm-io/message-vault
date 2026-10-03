import { type UIEvent, useCallback, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import Button from "../components/Button";
import ConfirmDialog from "../components/ConfirmDialog";
import ContactLabel from "../components/ContactLabel";
import { apiErrorMessage } from "../lib/apiErrorMessage";
import { contactLabelText } from "../lib/contactLabel";
import { NEAR_END_PX } from "../lib/listPaging";
import { keys } from "../lib/queryKeys";
import { type PagedFetchPage, useRoutePagedList, useRouteQuery } from "../lib/routeQuery";
import { unsupportedFieldWords, useSearchFields } from "../lib/searchFields";
import { trashed } from "../lib/searchQuery";
import { getConversation, listContacts, listConversations } from "../lib/serverApi";
import type { components } from "../lib/serverApi.types";
import {
  useDeleteContact,
  useDeleteConversation,
  useEmptyTrash,
  useRestoreContact,
  useRestoreConversation,
} from "../lib/trash";
import type { Conversation } from "../lib/types";
import { useAccountProfile } from "../lib/useAccountProfile";

/**
 * Trash holds two kinds of thing, and this pane is where both come back — or
 * leave for good. Trash is the only door to permanent deletion: Delete on a
 * row here and Empty Trash are the two places it exists, and lists elsewhere
 * offer Move to Trash alone.
 *
 * Trashed conversations are listed in the left column by the shared
 * conversation list, so this pane only reports how many there are and offers
 * Restore and Delete for the one the person selected. Trashed contacts have no
 * left column of their own — and a trashed contact cannot be opened at all,
 * since `GET /v1/contacts/{id}` filters them out — so they are listed here in
 * full, each row carrying its own Restore and Delete. That is why restoring a
 * contact lives on the row rather than in the contact drawer.
 *
 * Delete means two different things for the two kinds, and the dialogs say
 * which. A deleted conversation is gone with its messages. A deleted contact
 * loses its name and details and becomes Unknown; its messages stay, showing
 * the handle — the same thing Delete Contact does on a phone.
 *
 * Both lists read the same header search term, so narrowing Trash narrows both
 * kinds at once. A word only one list accepts (`participants:` is a
 * conversations word, `conversations:` a contacts word) is not sent to the
 * list that would refuse it; that pane says which list the word applies to
 * instead of showing the server's 422 Unprocessable Entity. A word neither
 * list accepts is named once, as a word that is not a search word, and
 * neither list is asked.
 */

/** How many trashed contacts this pane reads at a time; scrolling near the end reads the next page. */
const CONTACT_PAGE_SIZE = 100;

/** One trashed contact as the contact list answers it. */
type TrashedContact = components["schemas"]["ContactSummary"];

/** The `tsel` param as a positive conversation id, or null when absent or malformed. */
function selectedIdFromParam(raw: string | null): number | null {
  if (raw === null || !/^\d+$/.test(raw)) return null;
  const n = Number(raw);
  return Number.isSafeInteger(n) && n > 0 ? n : null;
}

/** Which confirmation is open, if any. */
type PendingDelete =
  | { kind: "conversation"; id: number; name: string; messageCount: number }
  | { kind: "contact"; id: number; name: string }
  | { kind: "empty" };

const sectionHeading =
  "m-0 mb-2 text-[0.75rem] font-semibold uppercase tracking-[0.04em] text-muted";

const errorBox =
  "mb-3 rounded border border-danger-soft-border bg-danger-soft-bg px-3 py-2 text-[0.813rem] text-danger";

const noteBox =
  "mb-3 rounded border border-border bg-elevated px-3 py-2 text-[0.813rem] text-muted";

/** Hover text on a disabled Delete when the account may not delete. */
const CANNOT_DELETE = "Deleting is not permitted for this account";

/** "re: is not a search word": the words neither list accepts. */
function notSearchWords(words: readonly string[]): string {
  const list = words.map((w) => `${w}:`).join(", ");
  return words.length === 1 ? `${list} is not a search word` : `${list} are not search words`;
}

/** "participants: applies to conversations only": the words a pane cannot answer, and who can. */
function appliesOnlyTo(words: readonly string[], list: "contacts" | "conversations"): string {
  const verb = words.length === 1 ? "applies" : "apply";
  return `${words.map((w) => `${w}:`).join(", ")} ${verb} to ${list} only`;
}

function plural(count: number, noun: string, many = `${noun}s`): string {
  return `${count} ${count === 1 ? noun : many}`;
}

export default function TrashScreen() {
  const [searchParams, setSearchParams] = useSearchParams();
  const search = searchParams.get("tq") || "";
  const query = trashed(search);
  const selectedId = selectedIdFromParam(searchParams.get("tsel"));

  // Which typed words each list refuses. The registry is fetched once per
  // session; until it arrives neither pane asks, so a refused word never
  // reaches the server as a 422. A failed request leaves an empty registry,
  // which would refuse every word, so no refusal is read from it.
  const conversationFields = useSearchFields("conversations");
  const contactFields = useSearchFields("contacts");
  const fieldsLoading = conversationFields.loading || contactFields.loading;
  const fieldsError = conversationFields.error ?? contactFields.error;
  const fieldsKnown = !fieldsLoading && fieldsError === null;
  const conversationsRefuse = fieldsKnown
    ? unsupportedFieldWords(query, conversationFields.fields)
    : [];
  const contactsRefuse = fieldsKnown ? unsupportedFieldWords(query, contactFields.fields) : [];
  // A word both lists refuse belongs to neither, so no pane says it is the
  // other list's word.
  const unknownWords = conversationsRefuse.filter((w) => contactsRefuse.includes(w));
  const contactsOnlyWords = conversationsRefuse.filter((w) => !unknownWords.includes(w));
  const conversationsOnlyWords = contactsRefuse.filter((w) => !unknownWords.includes(w));
  const askConversations = fieldsKnown && conversationsRefuse.length === 0;
  const askContacts = fieldsKnown && contactsRefuse.length === 0;

  // Only `total` is read here; the rows themselves are rendered by the list
  // column, so one row is enough to read `total` off the page response.
  const fetchCount = useCallback(
    async (signal: AbortSignal) => {
      const res = await listConversations({ q: query, limit: 1, offset: 0 }, { signal });
      return res.total ?? 0;
    },
    [query],
  );

  const {
    data,
    isPending: loading,
    error,
  } = useRouteQuery(keys.trash.count(query), fetchCount, { enabled: askConversations });

  const fetchContactPage = useCallback<PagedFetchPage<TrashedContact>>(
    ({ limit, offset, signal }) => listContacts({ q: query, limit, offset }, { signal }),
    [query],
  );
  const {
    items: contacts,
    total: contactTotal,
    loading: contactsLoading,
    filling: contactsFilling,
    error: contactsError,
    hasMore: moreContacts,
    loadMore: loadMoreContacts,
  } = useRoutePagedList(keys.contacts.trashed(query), fetchContactPage, {
    firstPageSize: CONTACT_PAGE_SIZE,
    fillPageSize: CONTACT_PAGE_SIZE,
    enabled: askContacts,
  });

  // AppLayout's left column sets `tsel` when a trashed conversation is clicked;
  // it stays on `/trash` rather than navigating to the thread, so this pane can
  // show Restore and Delete for the row the person just selected.
  const {
    data: selected,
    isPending: selectedLoading,
    error: selectedError,
  } = useRouteQuery(
    selectedId === null ? keys.trash.noSelection : keys.conversations.detail(selectedId),
    (signal) => getConversation(selectedId ?? 0, { signal }),
    { enabled: selectedId !== null },
  );

  const restoreContact = useRestoreContact();
  const deleteConversation = useDeleteConversation();
  const deleteContact = useDeleteContact();
  const emptyTrash = useEmptyTrash();

  // The server refuses a delete from an account without the delete grant (the
  // demo account, for one) with a 403; the buttons say so up front instead.
  // Until the profile has loaded the buttons stay live — the server is the
  // gate, this is only the explanation.
  const { profile } = useAccountProfile();
  const canDelete = profile?.can_delete ?? true;

  const [pending, setPending] = useState<PendingDelete | null>(null);
  const closeDialog = useCallback(() => {
    setPending(null);
    // A failure message belongs to the dialog it appeared in; the next one
    // starts clean.
    deleteConversation.reset();
    deleteContact.reset();
    emptyTrash.reset();
  }, [deleteConversation, deleteContact, emptyTrash]);

  // The row leaves the trashed list once restored or deleted, so drop the
  // selection along with it rather than pointing at a conversation this pane
  // can no longer show.
  const clearSelection = useCallback(() => {
    const next = new URLSearchParams(searchParams);
    next.delete("tsel");
    setSearchParams(next, { replace: true });
  }, [searchParams, setSearchParams]);

  if (fieldsLoading || (askConversations && loading) || (askContacts && contactsLoading))
    return <div className="p-6 text-[0.875rem] text-muted">Loading…</div>;

  const total = data ?? 0;
  const searching = search.trim().length > 0;
  // Trash is empty only when both lists answered and both answers are empty:
  // a list that was not asked, or failed, says nothing about what it holds.
  const nothingInTrash =
    data !== undefined &&
    error === null &&
    askContacts &&
    contactsError === null &&
    total === 0 &&
    contacts.length === 0 &&
    selectedId === null;
  // Empty Trash acts on all of Trash, so it is offered whenever Trash is not
  // known to be empty — a search that matches nothing says nothing about the
  // rest of it.
  const offerEmptyTrash = !nothingInTrash || searching;

  const selectedName =
    selected?.label ||
    (selected?.is_group
      ? `${selected.participants.length} participants`
      : selected?.participants[0]?.name) ||
    "this conversation";

  const confirmDelete = () => {
    if (pending === null) return;
    switch (pending.kind) {
      case "conversation":
        deleteConversation.mutate(pending.id, {
          onSuccess: () => {
            clearSelection();
            closeDialog();
          },
        });
        return;
      case "contact":
        deleteContact.mutate(pending.id, { onSuccess: closeDialog });
        return;
      case "empty":
        emptyTrash.mutate(undefined, {
          onSuccess: () => {
            clearSelection();
            closeDialog();
          },
        });
    }
  };

  const dialogBusy =
    deleteConversation.isPending || deleteContact.isPending || emptyTrash.isPending;
  const dialogError =
    pending?.kind === "conversation" && deleteConversation.error
      ? apiErrorMessage(deleteConversation.error, "Could not delete this conversation.")
      : pending?.kind === "contact" && deleteContact.error
        ? apiErrorMessage(deleteContact.error, "Could not delete this contact.")
        : pending?.kind === "empty" && emptyTrash.error
          ? apiErrorMessage(emptyTrash.error, "Could not empty Trash.")
          : "";

  return (
    <div className="max-w-[700px] p-6">
      <div className="mb-6 flex items-center justify-between gap-4">
        <h2 className="m-0">Trash</h2>
        {offerEmptyTrash && (
          <Button
            variant="danger"
            size="sm"
            disabled={!canDelete || dialogBusy}
            title={canDelete ? undefined : CANNOT_DELETE}
            onClick={() => setPending({ kind: "empty" })}
          >
            Empty Trash
          </Button>
        )}
      </div>
      {fieldsError ? (
        <div className={errorBox}>
          Could not load the search words. {apiErrorMessage(fieldsError, "")}
        </div>
      ) : unknownWords.length > 0 ? (
        <div className={noteBox} role="status">
          {notSearchWords(unknownWords)}
        </div>
      ) : nothingInTrash ? (
        <div className="text-[0.875rem] text-muted">
          {searching ? "Nothing in Trash matches this search." : "Trash is empty."}
        </div>
      ) : (
        <>
          <section className="mb-8">
            <h3 className={sectionHeading}>Conversations</h3>
            {error && (
              <div className={errorBox}>{apiErrorMessage(error, "Could not load Trash.")}</div>
            )}
            {selectedId !== null ? (
              selectedLoading ? (
                <div className="text-[0.875rem] text-muted">Loading…</div>
              ) : selected ? (
                // Keyed by the selection, so a Restore error or a Restore still
                // answering for the last conversation selected stays with it.
                <SelectedConversation
                  key={selectedId}
                  conversation={selected}
                  name={selectedName}
                  canDelete={canDelete}
                  dialogBusy={dialogBusy}
                  onRestored={clearSelection}
                  onDelete={() =>
                    setPending({
                      kind: "conversation",
                      id: selectedId,
                      name: selectedName,
                      messageCount: selected.message_count,
                    })
                  }
                />
              ) : (
                <div className="text-[0.875rem] text-danger">
                  {apiErrorMessage(selectedError, "Could not load this conversation.")}
                </div>
              )
            ) : contactsOnlyWords.length > 0 ? (
              <div className={noteBox} role="status">
                {appliesOnlyTo(contactsOnlyWords, "contacts")}
              </div>
            ) : error ? null : total === 0 ? (
              <div className="text-[0.875rem] text-muted">
                {searching ? "No conversations match this search." : "No conversations in Trash."}
              </div>
            ) : (
              <div className="text-[0.875rem] text-muted">
                {plural(total, "conversation")}
                {searching ? " matching this search" : ""} in Trash. Select one on the left to view
                it.
              </div>
            )}
          </section>

          <section>
            <h3 className={sectionHeading}>Contacts</h3>
            {contactsError && (
              <div className={errorBox}>
                {apiErrorMessage(contactsError, "Could not load trashed contacts.")}
              </div>
            )}
            {restoreContact.error && (
              <div className={errorBox}>
                {apiErrorMessage(restoreContact.error, "Could not restore this contact.")}
              </div>
            )}
            {conversationsOnlyWords.length > 0 ? (
              <div className={noteBox} role="status">
                {appliesOnlyTo(conversationsOnlyWords, "conversations")}
              </div>
            ) : contactsError ? null : contacts.length === 0 ? (
              <div className="text-[0.875rem] text-muted">
                {searching ? "No contacts match this search." : "No contacts in Trash."}
              </div>
            ) : (
              <>
                <div className="mb-2 text-[0.875rem] text-muted">
                  {plural(contactTotal, "contact")}
                  {searching ? " matching this search" : ""} in Trash
                  {moreContacts ? `, ${contacts.length} shown` : ""}.
                </div>
                <ul
                  aria-label="Contacts in Trash"
                  className="m-0 max-h-[32rem] list-none overflow-y-auto rounded border border-border bg-elevated p-0"
                  onScroll={(e: UIEvent<HTMLUListElement>) => {
                    const el = e.currentTarget;
                    if (el.scrollHeight - el.scrollTop - el.clientHeight <= NEAR_END_PX) {
                      loadMoreContacts();
                    }
                  }}
                >
                  {contacts.map((contact) => {
                    const restoring =
                      restoreContact.isPending && restoreContact.variables === contact.id;
                    return (
                      <li
                        key={contact.id}
                        className="flex items-center justify-between gap-4 border-0 border-b border-solid border-border px-4 py-3 last:border-b-0"
                      >
                        <div className="min-w-0">
                          <div className="truncate text-[0.875rem] text-text">
                            <ContactLabel name={contact.name} addresses={contact.addresses} />
                          </div>
                          <div className="text-[0.75rem] text-muted">
                            {plural(contact.identity_count, "identity", "identities")}
                          </div>
                        </div>
                        <div className="flex items-center gap-2">
                          <Button
                            variant="secondary"
                            size="sm"
                            // Every row's button reads "Restore", so the name it
                            // answers to says which contact it restores.
                            aria-label={`Restore ${contactLabelText(contact.name, contact.addresses)}`}
                            disabled={restoreContact.isPending || dialogBusy}
                            onClick={() => restoreContact.mutate(contact.id)}
                          >
                            {restoring ? "Restoring…" : "Restore"}
                          </Button>
                          <Button
                            variant="danger"
                            size="sm"
                            aria-label={`Delete ${contactLabelText(contact.name, contact.addresses)}`}
                            disabled={!canDelete || restoreContact.isPending || dialogBusy}
                            title={canDelete ? undefined : CANNOT_DELETE}
                            onClick={() =>
                              setPending({
                                kind: "contact",
                                id: contact.id,
                                name: contactLabelText(contact.name, contact.addresses),
                              })
                            }
                          >
                            Delete
                          </Button>
                        </div>
                      </li>
                    );
                  })}
                </ul>
                {contactsFilling ? (
                  <p className="mt-2 text-[0.813rem] text-muted">Loading more contacts…</p>
                ) : moreContacts ? (
                  <p className="mt-2 text-[0.813rem] text-muted">Scroll for more.</p>
                ) : null}
              </>
            )}
          </section>
        </>
      )}

      <ConfirmDialog
        open={pending?.kind === "conversation"}
        title="Delete this conversation?"
        body={
          pending?.kind === "conversation"
            ? `Deletes ${pending.name} and its ${plural(pending.messageCount, "message")} from your Message Crate. Attachments only these messages use go with them.`
            : ""
        }
        confirmLabel="Delete"
        danger
        busy={deleteConversation.isPending}
        error={dialogError}
        onClose={closeDialog}
        onConfirm={confirmDelete}
      />
      <ConfirmDialog
        open={pending?.kind === "contact"}
        title={pending?.kind === "contact" ? `Delete ${pending.name}?` : ""}
        body="The name and details go, and the contact becomes Unknown. The messages stay, showing the phone number or address instead."
        confirmLabel="Delete"
        danger
        busy={deleteContact.isPending}
        error={dialogError}
        onClose={closeDialog}
        onConfirm={confirmDelete}
      />
      <ConfirmDialog
        open={pending?.kind === "empty"}
        title="Empty Trash?"
        body={`Deletes every conversation in Trash with its messages and attachments. Every contact in Trash loses its name and details and becomes Unknown; their messages stay.${
          searching ? " This empties all of Trash, not only what matches the search." : ""
        }`}
        confirmLabel="Empty Trash"
        danger
        busy={emptyTrash.isPending}
        error={dialogError}
        onClose={closeDialog}
        onConfirm={confirmDelete}
      />
    </div>
  );
}

/** The trashed conversation selected in the list column, with Restore and Delete. */
function SelectedConversation({
  conversation,
  name,
  canDelete,
  dialogBusy,
  onRestored,
  onDelete,
}: {
  conversation: Conversation;
  name: string;
  canDelete: boolean;
  dialogBusy: boolean;
  onRestored: () => void;
  onDelete: () => void;
}) {
  const restoreConversation = useRestoreConversation();
  return (
    <div className="rounded border border-border bg-elevated p-4">
      <div className="mb-1 text-[0.938rem] font-semibold text-text">{name}</div>
      <div className="mb-3 text-[0.75rem] text-muted">
        {plural(conversation.message_count, "message")}
      </div>
      {restoreConversation.error && (
        <div className={errorBox}>
          {apiErrorMessage(restoreConversation.error, "Could not restore this conversation.")}
        </div>
      )}
      <div className="flex items-center gap-4">
        <Button
          variant="secondary"
          disabled={restoreConversation.isPending || dialogBusy}
          onClick={() => restoreConversation.mutate(conversation.id, { onSuccess: onRestored })}
        >
          {restoreConversation.isPending ? "Restoring…" : "Restore"}
        </Button>
        <Button
          variant="danger"
          disabled={!canDelete || restoreConversation.isPending || dialogBusy}
          title={canDelete ? undefined : CANNOT_DELETE}
          onClick={onDelete}
        >
          Delete
        </Button>
        <Link
          to={`/messages/${conversation.id}`}
          className="text-[0.875rem] text-accent underline-offset-2 hover:underline"
        >
          View conversation
        </Link>
      </div>
    </div>
  );
}

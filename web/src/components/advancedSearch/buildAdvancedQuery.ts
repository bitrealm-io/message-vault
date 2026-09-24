import {
  advancedContacts,
  advancedMessages,
  advancedTrash,
  type ContactsQueryInput,
  type CountFilterInput,
  composeCountComparison,
  type DateBoundFilter,
  type MessagesQueryInput,
} from "../../lib/searchQuery";

// The form's shapes are the builders' shapes: searchQuery.ts declares them
// once and this file passes them straight through, so a widened union or a
// new optional field cannot mean two different things on the two sides.
export type {
  ActivityFilter,
  ContactsQueryInput,
  CountComparator,
  CountFilterInput,
  DateBoundFilter,
  DateBoundOp,
  MessagesQueryInput,
} from "../../lib/searchQuery";
export { composeCountComparison };

/**
 * Which form the panel shows. `messages` is the conversation-list form,
 * `contacts` the contact-list form. `trash` is the form for a screen that
 * sends one query to both the contacts and the conversations list, so it may
 * only offer words both accept. The contacts form's `name:`, `handle:`,
 * `messages:`, and `service:` are registered on both lists, while the
 * messages form's `participants:` is conversations-only; so Trash shows the
 * contacts form, without its `first-heard:` and `last-heard:` dates, which
 * are Contacts words.
 */
export type AdvancedSearchMode = "messages" | "contacts" | "trash";

export const EMPTY_COUNT: CountFilterInput = { comparator: "any", value: "" };
export const EMPTY_DATE_BOUND: DateBoundFilter = { op: "any", start: "", end: "" };

export function dateBoundHasValue(bound: DateBoundFilter): boolean {
  if (bound.op === "any") return false;
  return Boolean(bound.start || (bound.op === "between" && bound.end));
}

export function buildMessagesQuery(input: MessagesQueryInput): string {
  return advancedMessages(input);
}

export function buildContactsQuery(input: ContactsQueryInput): string {
  return advancedContacts(input);
}

export function buildTrashQuery(input: ContactsQueryInput): string {
  return advancedTrash(input);
}

export function canSubmitMessages(input: MessagesQueryInput): boolean {
  return Boolean(
    input.nameOrHandle.trim() ||
      input.handle.trim() ||
      input.msgType !== "all" ||
      composeCountComparison(input.participants),
  );
}

export function canSubmitContacts(input: ContactsQueryInput): boolean {
  return Boolean(
    input.contactName.trim() ||
      input.handle.trim() ||
      dateBoundHasValue(input.firstHeardBound) ||
      dateBoundHasValue(input.lastHeardBound) ||
      input.activity !== "any" ||
      input.noPreferredName ||
      input.noHandle ||
      input.services.length > 0,
  );
}

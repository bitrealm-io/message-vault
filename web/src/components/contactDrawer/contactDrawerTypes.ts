import type { ContactDetail, ContactHandle } from "../../lib/contactDetail";
import { formatIsoDateOnly } from "../../lib/formatDate";
import type { ConversationKind } from "../../lib/searchQuery";

export type { HandleService } from "../../lib/handleService";
export { formatHandleServiceLabel, inferService } from "../../lib/handleService";

/** Lightweight row data so the drawer can paint before the detail API returns. */
export type ContactPreview = {
  id: string;
  name: string;
  handles?: string[];
  /**
   * True linked-identity count from the list API (`handle_count`).
   * List `handles` may include both raw and normalized forms of one identity;
   * stub rows while loading should match this count, not `handles.length`.
   */
  handleCount?: number;
  groups?: string[];
  /** True when the vault counts the contact in the Unknown Contact Group. */
  unknown?: boolean;
};

/** List-API contact row (snake_case `handle_count`) mapped into `ContactPreview`. */
export type ContactListPreviewSource = {
  id: string;
  name: string;
  handles?: string[];
  handle_count?: number;
  groups?: string[];
  unknown?: boolean;
};

/** Same three ways a set of conversations narrows by kind, named for this drawer. */
export type ContactBrowseKind = ConversationKind;

export function contactPreviewFromListRow(c: ContactListPreviewSource): ContactPreview {
  return {
    id: c.id,
    name: c.name,
    handles: c.handles,
    handleCount: c.handle_count,
    groups: c.groups,
    unknown: c.unknown,
  };
}

function sameStrings(a: string[] | undefined, b: string[] | undefined): boolean {
  if (a === b) return true;
  if (!a || !b || a.length !== b.length) return false;
  return a.every((value, i) => value === b[i]);
}

/**
 * Value equality for two preview lists. Callers hold these in state and re-map
 * them from list rows on every render, so comparing before storing keeps a
 * fresh-but-identical array from triggering another render pass.
 */
export function sameContactPreviews(
  a: readonly ContactPreview[],
  b: readonly ContactPreview[],
): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  return a.every((left, i) => {
    const right = b[i];
    return (
      left.id === right.id &&
      left.name === right.name &&
      left.handleCount === right.handleCount &&
      left.unknown === right.unknown &&
      sameStrings(left.handles, right.handles) &&
      sameStrings(left.groups, right.groups)
    );
  });
}

export type ThreadParticipantPreviewSource = {
  /** As the vault sends it: a number. The UI carries contact ids as strings. */
  contact_id?: number | null;
  /** Null/undefined when the source named this participant without an address. */
  handle?: string | null;
  name: string;
};

/** The participant's name, then handle — same order as chips. */
function threadParticipantDisplayName(p: ThreadParticipantPreviewSource): string {
  return p.name.trim() || p.handle?.trim() || "Contact";
}

export function contactPreviewFromThreadParticipants(
  contactId: string,
  participants: readonly ThreadParticipantPreviewSource[],
): ContactPreview | null {
  const matched = participants.filter(
    (p) => p.contact_id != null && String(p.contact_id) === contactId,
  );
  if (matched.length === 0) return null;
  const handles = matched.map((p) => p.handle).filter((h): h is string => !!h && h.length > 0);
  const named = matched.find((p) => Boolean(p.name.trim()));
  const uniqueCount = previewHandleStubRows(handles, undefined).length;
  return {
    id: contactId,
    name: threadParticipantDisplayName(named ?? matched[0]),
    handles,
    // At least one stub row so an empty handle list does not take the empty-table Loading path.
    handleCount: Math.max(1, uniqueCount),
  };
}

/** Format an API ISO timestamp as YYYY-MM-DD for the handles table. */
export function formatHandleDate(iso: string | null | undefined): string | null {
  return formatIsoDateOnly(iso);
}

export function emptyHandleRow(handle: string): ContactHandle {
  return {
    handle,
    service: "",
    start_date: null,
    end_date: null,
    conversations: 0,
    direct_messages: 0,
    group_messages: 0,
  };
}

/** Shown in the Identity cell when stubbing more rows than preview strings. */
export const HANDLE_STUB_PLACEHOLDER = "…";

/** Collapse raw/normalized forms of the same phone so stub labels stay unique. */
function handleStubKey(handle: string): string {
  const digits = handle.replace(/\D/g, "");
  return digits.length >= 7 ? digits : handle.trim().toLowerCase();
}

/**
 * Build loading stub rows for the handles table.
 * Prefer `handleCount` (one row per linked identity) over the full preview
 * string list, which can list both raw and normalized forms of the same phone.
 */
export function previewHandleStubRows(
  handles: string[] | undefined,
  handleCount: number | undefined,
): ContactHandle[] {
  const unique: string[] = [];
  const seen = new Set<string>();
  for (const handle of handles ?? []) {
    const key = handleStubKey(handle);
    if (seen.has(key)) continue;
    seen.add(key);
    unique.push(handle);
  }
  const count =
    handleCount != null && Number.isFinite(handleCount)
      ? Math.max(0, Math.floor(handleCount))
      : unique.length;
  const rows: ContactHandle[] = [];
  for (let i = 0; i < count; i++) {
    rows.push(emptyHandleRow(unique[i] ?? HANDLE_STUB_PLACEHOLDER));
  }
  return rows;
}

/** The earliest, the latest, and the sums across a contact's identities. */
export function sumHandleTotals(handles: ContactDetail["handles"]): {
  conversations: number;
  direct_messages: number;
  group_messages: number;
  start_date: string | null;
  end_date: string | null;
} {
  let conversations = 0;
  let direct_messages = 0;
  let group_messages = 0;
  let start_date: string | null = null;
  let end_date: string | null = null;
  for (const h of handles) {
    conversations += h.conversations;
    direct_messages += h.direct_messages;
    group_messages += h.group_messages;
    if (h.start_date && (!start_date || h.start_date < start_date)) start_date = h.start_date;
    if (h.end_date && (!end_date || h.end_date > end_date)) end_date = h.end_date;
  }
  return { conversations, direct_messages, group_messages, start_date, end_date };
}

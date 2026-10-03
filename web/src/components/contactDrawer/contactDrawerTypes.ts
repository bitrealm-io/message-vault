import type { ContactDetail, ContactHandle } from "../../lib/contactDetail";
import { formatIsoDateOnly } from "../../lib/formatDate";
import type { ConversationKind } from "../../lib/searchQuery";

export type { HandleService } from "../../lib/handleService";
export { formatHandleServiceLabel, inferService } from "../../lib/handleService";

/** Lightweight row data so the drawer can paint before the detail API returns. */
export type ContactPreview = {
  id: string;
  name: string;
  addresses?: string[];
  /**
   * True linked-identity count from the list API (`identity_count`).
   * List `addresses` may include both raw and normalized forms of one identity;
   * stub rows while loading should match this count, not `addresses.length`.
   */
  handleCount?: number;
  groups?: string[];
  /** True when the server counts the contact in the Unknown Contact Group. */
  unknown?: boolean;
};

/** List-API contact row (snake_case `identity_count`) mapped into `ContactPreview`. */
export type ContactListPreviewSource = {
  id: string;
  name: string;
  addresses?: string[];
  identity_count?: number;
  groups?: string[];
  unknown?: boolean;
};

/** Same three ways a set of conversations narrows by kind, named for this drawer. */
export type ContactBrowseKind = ConversationKind;

export function contactPreviewFromListRow(c: ContactListPreviewSource): ContactPreview {
  return {
    id: c.id,
    name: c.name,
    addresses: c.addresses,
    handleCount: c.identity_count,
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
      sameStrings(left.addresses, right.addresses) &&
      sameStrings(left.groups, right.groups)
    );
  });
}

export type ThreadParticipantPreviewSource = {
  /** As the server sends it: a number. The UI carries contact ids as strings. */
  contact_id?: number | null;
  /** Null/undefined when the source named this participant without an address. */
  identity?: string | null;
  name: string;
};

/** The participant's name, then identity — same order as chips. */
function threadParticipantDisplayName(p: ThreadParticipantPreviewSource): string {
  return p.name.trim() || p.identity?.trim() || "Contact";
}

export function contactPreviewFromThreadParticipants(
  contactId: string,
  participants: readonly ThreadParticipantPreviewSource[],
): ContactPreview | null {
  const matched = participants.filter(
    (p) => p.contact_id != null && String(p.contact_id) === contactId,
  );
  if (matched.length === 0) return null;
  const addresses = matched.map((p) => p.identity).filter((h): h is string => !!h && h.length > 0);
  const named = matched.find((p) => Boolean(p.name.trim()));
  const uniqueCount = previewHandleStubRows(addresses, undefined).length;
  return {
    id: contactId,
    name: threadParticipantDisplayName(named ?? matched[0]),
    addresses,
    // At least one stub row so an empty handle list does not take the empty-table Loading path.
    handleCount: Math.max(1, uniqueCount),
  };
}

/** Format an API ISO timestamp as YYYY-MM-DD in `zone` for the handles table. */
export function formatHandleDate(iso: string | null | undefined, zone: string): string | null {
  return formatIsoDateOnly(iso, zone);
}

export function emptyHandleRow(address: string): ContactHandle {
  return {
    address,
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
  addresses: string[] | undefined,
  handleCount: number | undefined,
): ContactHandle[] {
  const unique: string[] = [];
  const seen = new Set<string>();
  for (const handle of addresses ?? []) {
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

/**
 * A contact's totals: its conversations as the server counts them, once each,
 * and the earliest, the latest, and the message sums across its identities.
 * Conversations are not summed, because one conversation can hold two of the
 * identities and each identity counts it; a message has one sender, so the
 * message counts add up.
 */
export function contactTotals(detail: ContactDetail): {
  conversations: number;
  direct_messages: number;
  group_messages: number;
  start_date: string | null;
  end_date: string | null;
} {
  const conversations = detail.direct_conversations + detail.group_conversations;
  let direct_messages = 0;
  let group_messages = 0;
  let start_date: string | null = null;
  let end_date: string | null = null;
  for (const h of detail.identities) {
    direct_messages += h.direct_messages;
    group_messages += h.group_messages;
    if (h.start_date && (!start_date || h.start_date < start_date)) start_date = h.start_date;
    if (h.end_date && (!end_date || h.end_date > end_date)) end_date = h.end_date;
  }
  return { conversations, direct_messages, group_messages, start_date, end_date };
}

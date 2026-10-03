import {
  createNameCollection,
  useNameCollectionActions,
  useSetNamedSetMembers,
} from "./nameCollection";
import { keys } from "./queryKeys";
import { forGroup } from "./searchQuery";
import {
  createContactGroup,
  deleteContactGroup,
  listContactGroups,
  updateContactGroup,
  updateContactGroupMembers,
} from "./serverApi";
import { UNKNOWN_GROUP } from "./unknownGroup";

/** Names that must not be created as user groups. */
export const RESERVED_GROUP_NAMES = new Set(
  [
    "home",
    "contacts",
    "all",
    "excluded",
    "no-messages",
    "no messages",
    "unassigned",
    "trash",
    "groups",
    "group",
    "group-chats",
    "group chats",
    "group-chats-2",
    "group chats 2",
    "group-messages",
    "group messages",
    "group-messages-2",
    "group messages 2",
    "no-label",
    "no-group",
    "no group",
    "labels",
    "label",
    "no label",
    // The two computed groups, as `group:` names them.
    "unknown",
    "none",
  ].map((s) => s.toLowerCase()),
);

export function reservedGroupError(name: string): string {
  const key = name.trim().toLowerCase();
  if (key === "contacts") return "Contacts is a reserved Contact Group";
  if (key === "all") return "All is a reserved Contact Group";
  if (key === "excluded") return "Excluded is a reserved Contact Group";
  if (key === "unassigned") return "Unassigned is a reserved Contact Group";
  if (key === "trash") return "Trash is a reserved Contact Group";
  if (key === "no messages" || key === "no-messages") {
    return "No messages is a reserved Contact Group";
  }
  return `"${name.trim()}" is a reserved Contact Group`;
}

export const contactGroups = createNameCollection({
  routes: {
    list: listContactGroups,
    create: createContactGroup,
    update: updateContactGroup,
    remove: deleteContactGroup,
    updateMembers: updateContactGroupMembers,
  },
  key: keys.contactGroups.all,
  // A ticked box shows on the contact rows and on the open contact at once.
  chips: [
    { key: keys.contacts.lists, field: "groups", shape: "pages" },
    { key: keys.contacts.details, field: "groups", shape: "row" },
  ],
  label: "group",
  forName: forGroup,
  reservedNames: RESERVED_GROUP_NAMES,
  reservedError: reservedGroupError,
});

export function isReservedGroupName(name: string): boolean {
  return contactGroups.isReserved(name);
}

/**
 * What a group's route holds for its name: the whole name, trimmed. Letter
 * case, spaces and punctuation stay, so every name has a route and no two
 * names share one. `slugPath` percent-encodes it into the path.
 */
export function groupSlug(name: string): string {
  return name.trim();
}

/** Find the group whose name is this slug, exactly and then ignoring case, or null. */
export function groupFromSlug(slug: string, groups: readonly string[]): string | null {
  const trimmed = slug.trim();
  if (!trimmed) return null;
  for (const name of groups) {
    if (groupSlug(name) === trimmed) return name;
  }
  const folded = trimmed.toLowerCase();
  for (const name of groups) {
    if (groupSlug(name).toLowerCase() === folded) return name;
  }
  return null;
}

/** The path of one set's page, e.g. `/group/Work%20Friends` for "Work Friends". */
export function slugPath(routeBase: string, slug: string): string {
  return `${routeBase}/${encodeURIComponent(slug)}`;
}

/**
 * The slug a set page's path holds, decoded, or null when the path is not
 * under `routeBase`. A malformed escape is read as written, since
 * `decodeURIComponent` throws on it.
 */
export function slugFromPath(pathname: string, routeBase: string): string | null {
  const prefix = `${routeBase}/`;
  if (!pathname.startsWith(prefix)) return null;
  const raw = pathname.slice(prefix.length);
  try {
    return decodeURIComponent(raw);
  } catch {
    return raw;
  }
}

/**
 * True when this contact should appear on the given group page. This is the
 * server's rule for `group:`, applied to rows already in memory.
 */
export function contactBelongsToGroup(
  contact: { groups?: readonly string[]; unknown: boolean },
  groupFilter: string | "none" | null,
): boolean {
  if (!groupFilter) return true;
  // Unknown is computed by the server from contact state, so it never appears
  // in a contact's stored group names. Each row carries the answer instead.
  if (groupFilter === UNKNOWN_GROUP) return contact.unknown;
  const groups = contact.groups ?? [];
  // An Unknown contact is in the Unknown group, so it is not in No group.
  if (groupFilter === "none") return groups.length === 0 && !contact.unknown;
  const needle = groupFilter.toLowerCase();
  return groups.some((g) => g.toLowerCase() === needle);
}

/** Build the contact-list query for a group page plus optional typed search. */
export const groupListQuery = contactGroups.listQuery;

/** Create, rename, delete, and set membership on Contact Groups. */
export function useContactGroupActions() {
  return useNameCollectionActions(contactGroups);
}

/** Put contacts in or out of one Contact Group, drawn before the server answers. */
export function useSetContactGroupMembers() {
  return useSetNamedSetMembers(contactGroups);
}

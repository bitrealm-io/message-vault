import { groupFromSlug, groupSlug } from "./contactGroups";
import {
  createNameCollection,
  useNameCollectionActions,
  useSetNamedSetMembers,
} from "./nameCollection";
import { keys } from "./queryKeys";
import { forTag } from "./searchQuery";
import {
  createMessageTag,
  deleteMessageTag,
  listMessageTags,
  updateMessageTag,
  updateMessageTagMembers,
} from "./serverApi";

/** Names that must not be created as message tags. */
export const RESERVED_TAG_NAMES = new Set(
  [
    "home",
    "contacts",
    "threads",
    "thread",
    "all",
    "excluded",
    "unassigned",
    "trash",
    "tags",
    "tag",
    "no-tag",
    "no tag",
    "groups",
    "group",
    "labels",
    "label",
  ].map((s) => s.toLowerCase()),
);

export function reservedTagError(name: string): string {
  return `"${name.trim()}" is a reserved tag`;
}

export const messageTags = createNameCollection({
  routes: {
    list: listMessageTags,
    create: createMessageTag,
    update: updateMessageTag,
    remove: deleteMessageTag,
    updateMembers: updateMessageTagMembers,
  },
  key: keys.messageTags.all,
  // Conversation rows and the Trash count show tag names.
  invalidates: [keys.conversations.all, keys.trash.all],
  chips: [{ key: keys.conversations.lists, field: "tags", shape: "pages" }],
  label: "tag",
  forName: forTag,
  reservedNames: RESERVED_TAG_NAMES,
  reservedError: reservedTagError,
});

export function isReservedTagName(name: string): boolean {
  return messageTags.isReserved(name);
}

// Slugs are URL syntax, not vocabulary — tags and groups share one rule.
export const tagSlug = groupSlug;
export const tagFromSlug = groupFromSlug;

/** Build the thread-list query for a tag page plus optional typed search. */
export const tagListQuery = messageTags.listQuery;

/** Create, rename, delete, and set membership on Message Tags. */
export function useMessageTagActions() {
  return useNameCollectionActions(messageTags);
}

/** Put conversations in or out of one Message Tag, drawn before the vault answers. */
export function useSetMessageTagMembers() {
  return useSetNamedSetMembers(messageTags);
}

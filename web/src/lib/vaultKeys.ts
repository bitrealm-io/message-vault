/**
 * Every cache key the web app uses, in one place.
 *
 * A key is built here and nowhere else, for one reason: TanStack Query marks
 * entries stale by prefix, so a write can only say "everything about contacts
 * is stale" if something owns the word `contacts`. When keys were literals
 * typed at each call site, that knowledge lived in comments, and screens kept
 * their own override maps rather than trusting an invalidation they could not
 * name.
 *
 * One rule, for every resource: a namespace whose `all` is the prefix, with
 * the builders nested under it. The account is not here — `vaultQueryKey` puts
 * it in front of whatever these produce, so no key in this file is complete on
 * its own. See `docs/adr/0002-one-way-to-fetch-data-in-the-web-app.md`.
 */

/** What makes one page of the conversation list its own cache entry. */
export type ConversationListKey = { q: string; sort: string; order: string };

export const keys = {
  contacts: {
    /** Every contact list page and every open contact. */
    all: ["contacts"] as const,
    /** The list pages only, leaving an open drawer's entry alone. */
    lists: ["contacts", "list"] as const,
    list: (q: string) => ["contacts", "list", q] as const,
    details: ["contacts", "detail"] as const,
    /** Ids arrive as numbers from the vault and as strings from the router. */
    detail: (id: string | number) => ["contacts", "detail", String(id)] as const,
    /**
     * The trashed contacts the Trash screen lists.
     *
     * A builder of its own rather than `list("trashed:yes …")` because the
     * contact list screen holds that entry as TanStack Query's paged
     * `InfiniteData` and the Trash screen holds a single page, and two shapes
     * must not share a key. It still sits under the `lists` prefix, so the
     * contact trash mutations mark it stale along with everything else that
     * lists contacts.
     */
    trashed: (q: string) => ["contacts", "list", "trashed", q] as const,
  },
  conversations: {
    all: ["conversations"] as const,
    lists: ["conversations", "list"] as const,
    list: ({ q, sort, order }: ConversationListKey) =>
      ["conversations", "list", q, sort, order] as const,
    detail: (id: number) => ["conversations", "detail", String(id)] as const,
    messages: (id: number, p: { offset: number; limit: number; year: number | null }) =>
      ["conversations", "messages", String(id), p.offset, p.limit, String(p.year)] as const,
    /** Every find inside any conversation, for the trash mutations to mark stale. */
    finds: ["conversations", "find"] as const,
    /** The messages matching the find box inside one conversation: `GET /v1/messages?q=in:#id …`. */
    find: (id: number, q: string, offset: number, limit: number) =>
      ["conversations", "find", String(id), q, offset, limit] as const,
    sources: (id: number | null) => ["conversations", "sources", String(id)] as const,
  },
  contactGroups: { all: ["contact-groups"] as const },
  messageTags: { all: ["message-tags"] as const },
  savedSearches: { all: ["saved-searches"] as const },
  searchFields: {
    all: ["search-fields"] as const,
    list: (list: string) => ["search-fields", list] as const,
  },
  accountProfile: { all: ["account-profile"] as const },
  apiTokens: { all: ["api-tokens"] as const },
  /** The accounts the vault owner manages, and the vault's own settings. */
  ownerAccounts: {
    all: ["owner-accounts"] as const,
    /** One account the owner has opened. Under `all`, so a write to the list refreshes it too. */
    member: (accountId: number) => ["owner-accounts", accountId] as const,
    storage: (accountId: number) => ["owner-accounts", accountId, "storage"] as const,
  },
  vaultSettings: { all: ["vault-settings"] as const },
  /** The vault's own Build and Schema Fingerprint, from `GET /v1/vault`. */
  vaultInfo: { all: ["vault-info"] as const },
  storage: {
    all: ["storage"] as const,
    overview: ["storage", "overview"] as const,
    importDetail: (id: number | null) => ["storage", "import", String(id)] as const,
  },
  trash: {
    all: ["trash"] as const,
    count: (q: string) => ["trash", "count", q] as const,
    /**
     * Stands in for the conversation-detail key while nothing is selected in
     * Trash. The query is disabled either way, but naming a real conversation
     * id — `detail(0)` — would put an entry in the cache that looks like a
     * conversation nobody asked for.
     */
    noSelection: ["trash", "no-selection"] as const,
  },
};

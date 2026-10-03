/**
 * Every cache key the web app uses, in one place.
 *
 * A key is built here and nowhere else, for one reason: TanStack Query finds
 * entries by prefix, so an optimistic write can only patch "every contact
 * list" if something owns the words `contacts` and `list`. When keys were
 * literals typed at each call site, that knowledge lived in comments, and
 * screens kept their own override maps rather than trusting a prefix they
 * could not name. Marking entries stale needs no prefix from here: every
 * write marks the whole account's cache stale (`invalidateAccount` in
 * `routeQuery.ts`).
 *
 * One rule, for every resource: a namespace whose `all` is the prefix, with
 * the builders nested under it. The account is not here — `routeQueryKey` puts
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
    /** Ids arrive as numbers from the server and as strings from the router. */
    detail: (id: string | number) => ["contacts", "detail", String(id)] as const,
    /**
     * The trashed contacts the Trash screen lists.
     *
     * A builder of its own rather than `list("trashed:yes …")` because the
     * Trash screen pages its list in pages of its own size, and two lists
     * paged differently must not share an entry. It still sits under the
     * `lists` prefix, with everything else that lists contacts.
     */
    trashed: (q: string) => ["contacts", "list", "trashed", q] as const,
    /**
     * The figures `POST /v1/contacts/summaries` returns for the checked
     * contacts, one entry per set of ids. Outside `lists`, whose entries are
     * pages of contacts and are patched as such, and under `all`, so a write
     * that marks every contact stale refreshes these figures too.
     */
    summaries: (ids: readonly string[]) => ["contacts", "summaries", ids.join(",")] as const,
  },
  conversations: {
    all: ["conversations"] as const,
    lists: ["conversations", "list"] as const,
    list: ({ q, sort, order }: ConversationListKey) =>
      ["conversations", "list", q, sort, order] as const,
    detail: (id: number) => ["conversations", "detail", String(id)] as const,
    messages: (id: number, p: { offset: number; limit: number }) =>
      ["conversations", "messages", String(id), p.offset, p.limit] as const,
    /** One conversation narrowed to a year or a find term: `GET /v1/messages?q=in:#id …`. */
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
  accountProfile: {
    all: ["account-profile"] as const,
    /** The logged-in account's identities with their message counts. */
    identities: ["account-profile", "identities"] as const,
  },
  apiTokens: { all: ["api-tokens"] as const },
  /** The accounts the owner manages. */
  ownerAccounts: {
    all: ["owner-accounts"] as const,
    /** One account the owner has opened. */
    member: (accountId: number) => ["owner-accounts", accountId] as const,
    storage: (accountId: number) => ["owner-accounts", accountId, "storage"] as const,
    identities: (accountId: number) => ["owner-accounts", accountId, "identities"] as const,
    /** One page of the account's Import Runs. Under `storage`, like the run it opens. */
    imports: (accountId: number, page: number) =>
      ["owner-accounts", accountId, "storage", "imports", page] as const,
    /** One page of the account's Export Runs. */
    exports: (accountId: number, page: number) =>
      ["owner-accounts", accountId, "storage", "exports", page] as const,
    importDetail: (accountId: number, id: number | null) =>
      ["owner-accounts", accountId, "storage", "import", String(id)] as const,
  },
  imports: {
    all: ["imports"] as const,
    /**
     * The account's running Import Run, or null. The Import screen's resume
     * check and the sidebar's Import badge read this one entry, so the two
     * cannot disagree about whether a run is waiting.
     */
    running: ["imports", "running"] as const,
  },
  serverSettings: { all: ["server-settings"] as const },
  /** Where the Demo Account stands, from `GET /v1/server/demo-account`. */
  demoAccount: { all: ["demo-account"] as const },
  /** What the whole database holds, from `GET /v1/server/storage`. */
  serverStorage: { all: ["server-storage"] as const },
  /** The server's own Build and Schema Fingerprint, from `GET /v1/server`. */
  serverInfo: { all: ["server-info"] as const },
  storage: {
    all: ["storage"] as const,
    overview: ["storage", "overview"] as const,
    /** One page of the account's Import Runs. */
    imports: (page: number) => ["storage", "imports", page] as const,
    /** One page of the account's Export Runs. */
    exports: (page: number) => ["storage", "exports", page] as const,
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

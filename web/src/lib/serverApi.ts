/**
 * Every server route the web app calls, one named function each.
 *
 * This is the only module that knows a server URL. Screens call
 * `listConversations` rather than writing `/v1/conversations?…`, so renaming a
 * route is a change here and nowhere else, and no test has to match on a path.
 *
 * Request and response types come from `serverApi.types.ts`, which is generated
 * from `docs/src/assets/openapi.json` — the document a server-side test pins to
 * the running server. Regenerate with `npm run gen:api`;
 * `scripts/check-generated-api-types.sh`, run by `scripts/check-all.sh` and CI,
 * fails when the checked-in file is out of date.
 *
 * These functions only talk to the server. Caching, request deduplication, and
 * telling the rest of the app that something changed all belong to TanStack
 * Query above this layer. See
 * `docs/adr/0002-one-way-to-fetch-data-in-the-web-app.md`.
 *
 * Routes reachable only from the desktop app's Rust side — asset upload and
 * `POST /v1/imports/{id}/batches` — have no function here, because nothing
 * in the browser calls them.
 */

import {
  type ApiRequestOptions,
  apiClient,
  getAccountId,
  getBaseUrl,
  getToken,
  problemFromBody,
} from "./api";
import { buildAssetPath, buildAssetPreviewPath } from "./assetUrl";
import { PAGE_SIZE_MAX } from "./listPaging";
import type { components, paths } from "./serverApi.types";

type Schema = components["schemas"];
/** What `GET /v1/server` answers, named by its route so a renamed schema changes nothing here. */
type Server = paths["/v1/server"]["get"]["responses"][200]["content"]["application/json"];

/** Options every read accepts, so a caller can cancel an in-flight request. */
export type RequestOptions = ApiRequestOptions;

/**
 * Build a query string from values that may be absent.
 *
 * Keys whose value is `undefined`, `null`, or an empty string are dropped, so
 * a caller can pass its whole filter object without pruning it first. The
 * result has no leading `?`; callers that need one add it.
 */
function query(params: Record<string, string | number | boolean | undefined | null>): string {
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === null || value === "") continue;
    search.set(key, String(value));
  }
  return search.toString();
}

/**
 * Every row of a paged list, read in pages of the server's maximum.
 *
 * Every `/v1` list answers one page, and 40 rows when no `limit` is sent, so
 * a caller that takes one answer as the whole list loses every row past the
 * page (issue #1145). This asks for the next page until the rows read reach
 * the `total` the last page reported. An empty page ends the reading too,
 * because a list that shrank between two requests reports a total it no
 * longer has.
 */
async function readEveryPage<Page extends { items: unknown[]; total: number }>(
  path: string,
  opts?: RequestOptions,
): Promise<Page["items"]> {
  const rows: Page["items"] = [];
  for (;;) {
    const page = await apiClient.get<Page>(
      withQuery(path, query({ limit: PAGE_SIZE_MAX, offset: rows.length })),
      opts,
    );
    rows.push(...page.items);
    if (page.items.length === 0 || rows.length >= page.total) return rows;
  }
}

/** Append a query string only when it has something in it. */
function withQuery(path: string, qs: string): string {
  return qs ? `${path}?${qs}` : path;
}

/** `/v1/accounts/{id}` for one account. */
function accountPath(accountId: number): string {
  return `/v1/accounts/${accountId}`;
}

/**
 * `/v1/accounts/{id}` for the logged-in account.
 *
 * The server has no `/v1/account` singleton: an account reads and writes its
 * own row in the same collection the owner manages, addressed by the id the
 * session carries. Logged out, there is no such row, and asking for one is a
 * bug in the caller rather than a request worth sending.
 */
function ownAccountPath(): string {
  const id = getAccountId();
  if (id === null) throw new Error("Not logged in");
  return accountPath(id);
}

// ── Auth ────────────────────────────────────────────────────────────────────

/** Log in. The Session is a singleton, so the server answers `201` with `Location: /v1/session`. */
export function login(
  body: Schema["CreateSessionRequest"],
): Promise<Schema["CreateSessionResponse"]> {
  return apiClient.post<Schema["CreateSessionResponse"]>("/v1/session", body);
}

/** The Session the bearer token names: its account, username, and import sources. */
export function getSession(opts?: RequestOptions): Promise<Schema["Session"]> {
  return apiClient.get<Schema["Session"]>("/v1/session", opts);
}

/** Log out: end the Session. The server answers `204`. */
export function logout(opts?: RequestOptions): Promise<void> {
  return apiClient.delete<void>("/v1/session", undefined, opts);
}

// ── The server itself ────────────────────────────────────────────────────────

/**
 * What state this Message Crate is in, for the screen a logged-out visitor sees.
 *
 * The server reports one value rather than the facts behind it, so the rule
 * joining "does an owner exist" to "is registration open" is stated once, on
 * the server. See `docs/adr/0008-the-owner-holds-no-messages.md`.
 */
export function getServerState(opts?: RequestOptions): Promise<Server> {
  return apiClient.get<Server>("/v1/server", opts);
}

/** Claim an unclaimed Message Crate by creating its owner. Returns their session. */
export function claimServer(
  body: Schema["ClaimServerRequest"],
): Promise<Schema["CreateSessionResponse"]> {
  return apiClient.post<Schema["CreateSessionResponse"]>("/v1/server/claim", body);
}

// ── The accounts collection ─────────────────────────────────────────────────
//
// One collection for the owner and for each account: the owner reaches
// every row, an account reaches its own. The functions Owner Home calls take
// the account id; the ones Settings calls address the logged-in
// account through `ownAccountPath`.

/** Every account of this Message Crate, for the owner: the owner's own first, then the rest by username. */
export function listAccounts(opts?: RequestOptions): Promise<Schema["Account"][]> {
  return readEveryPage<Schema["Page_Account"]>("/v1/accounts", opts);
}

/**
 * Create an account.
 *
 * Logged out, on an open Message Crate, this is registration: the server opens a
 * Session on the new account and answers its `token`. Logged in as the owner,
 * it creates an account whose holder must replace the password at first
 * login, and no session is opened.
 */
export function createAccount(
  body: Schema["CreateAccountRequest"],
): Promise<Schema["CreateAccountResponse"]> {
  return apiClient.post<Schema["CreateAccountResponse"]>("/v1/accounts", body);
}

/** One account, as the owner: profile, flags, and how much it holds. */
export function getAccount(accountId: number, opts?: RequestOptions): Promise<Schema["Account"]> {
  return apiClient.get<Schema["Account"]>(accountPath(accountId), opts);
}

/** Change an account's disabled flag or its import, export and delete grants, as the owner. */
export function updateAccount(
  accountId: number,
  body: Schema["UpdateAccountRequest"],
): Promise<Schema["Account"]> {
  return apiClient.patch<Schema["Account"]>(accountPath(accountId), body);
}

/** Set another account's password as the owner, ending its sessions. */
export function setAccountPassword(
  accountId: number,
  body: Schema["ReplaceAccountPasswordRequest"],
): Promise<void> {
  return apiClient.put<void>(`${accountPath(accountId)}/password`, body);
}

/** Delete an account as the owner: its login, profile, contacts, and every message it owns. */
export function deleteAccountById(accountId: number): Promise<void> {
  return apiClient.delete<void>(accountPath(accountId));
}

/** Destroy one account's messages as the owner. The account, its contacts and login survive. */
export function deleteAccountMessages(accountId: number): Promise<unknown> {
  return apiClient.delete<unknown>(`${accountPath(accountId)}/messages`);
}

/** Settings that belong to the whole Message Crate. */
export function getServerSettings(opts?: RequestOptions): Promise<Schema["ServerSettings"]> {
  return apiClient.get<Schema["ServerSettings"]>("/v1/server/settings", opts);
}

/**
 * What the whole database holds, summed over every account: message,
 * conversation, contact and attachment counts, and attachment bytes. The
 * owner's, and counts only (`docs/adr/0008-the-owner-holds-no-messages.md`).
 */
export function getServerStorage(opts?: RequestOptions): Promise<Schema["ServerStorage"]> {
  return apiClient.get<Schema["ServerStorage"]>("/v1/server/storage", opts);
}

/** Change the server's settings. Omitted fields are left alone. */
export function updateServerSettings(
  body: Schema["UpdateServerSettingsRequest"],
): Promise<Schema["ServerSettings"]> {
  return apiClient.patch<Schema["ServerSettings"]>("/v1/server/settings", body);
}

/** Where the Demo Account stands: absent, building, ready, or failed. */
export function getDemoAccount(opts?: RequestOptions): Promise<Schema["DemoAccount"]> {
  return apiClient.get<Schema["DemoAccount"]>("/v1/server/demo-account", opts);
}

/**
 * Add the Demo Account, or reset it. The server answers at once with
 * `building` and builds it afterwards; `getDemoAccount` says when it ends.
 */
export function replaceDemoAccount(
  body: Schema["ReplaceDemoAccountRequest"],
): Promise<Schema["DemoAccount"]> {
  return apiClient.put<Schema["DemoAccount"]>("/v1/server/demo-account", body);
}

// ── The logged-in account's own row ─────────────────────────────────────────

/** The logged-in account: profile, flags, and how much it holds. */
export function getAccountProfile(opts?: RequestOptions): Promise<Schema["Account"]> {
  return apiClient.get<Schema["Account"]>(ownAccountPath(), opts);
}

/** Change the logged-in account’s display name, time zone or identities. */
export function updateAccountProfile(
  body: Schema["UpdateAccountRequest"],
): Promise<Schema["Account"]> {
  return apiClient.patch<Schema["Account"]>(ownAccountPath(), body);
}

/** Change the logged-in account's own password. The server answers a rotated session token. */
export function changePassword(
  body: Schema["ReplaceAccountPasswordRequest"],
): Promise<Schema["ReplaceAccountPasswordResponse"]> {
  return apiClient.put<Schema["ReplaceAccountPasswordResponse"]>(
    `${ownAccountPath()}/password`,
    body,
  );
}

/** Delete the logged-in account, confirming with its current password. */
export function deleteAccount(body: Schema["DeleteAccountRequest"]): Promise<void> {
  return apiClient.delete<void>(ownAccountPath(), body);
}

/** How much an account holds: the logged-in one, or as the owner the one named. */
export function getAccountStorage(
  opts?: RequestOptions,
  accountId?: number,
): Promise<Schema["AccountStorage"]> {
  return apiClient.get<Schema["AccountStorage"]>(`${accountBase(accountId)}/storage`, opts);
}

// An account's import and export history, for its Storage screen. These are
// not `/v1/imports` and `/v1/exports`: those are the pipelines' own routes and
// ask for a permission, which the owner's session never carries.

function accountBase(accountId?: number): string {
  return accountId === undefined ? ownAccountPath() : accountPath(accountId);
}

/** Every identity of an account with the messages held at each: the logged-in one, or as the owner the one named. */
export function listAccountIdentities(
  opts?: RequestOptions,
  accountId?: number,
): Promise<Schema["Identity"][]> {
  const path = `${accountBase(accountId)}/identities`;
  return readEveryPage<Schema["Page_Identity"]>(path, opts);
}

/** Which page of an account's run history to read. Absent values are left off the URL. */
export type AccountRunListParams = { limit?: number; offset?: number };

/**
 * An account's Import Runs, newest first: the logged-in one in full, or as the
 * owner the one named, each without what the run held.
 */
export function listAccountImports(
  params: AccountRunListParams,
  opts?: RequestOptions,
  accountId?: number,
): Promise<Schema["AccountImportRuns"]> {
  return apiClient.get<Schema["AccountImportRuns"]>(
    withQuery(`${accountBase(accountId)}/imports`, query(params)),
    opts,
  );
}

/**
 * One of an account's Import Runs, with its counts and timings, and for the
 * logged-in account its summary and issues.
 */
export function getAccountImport(
  importId: number,
  opts?: RequestOptions,
  accountId?: number,
): Promise<Schema["AccountImportRun"]> {
  return apiClient.get<Schema["AccountImportRun"]>(
    `${accountBase(accountId)}/imports/${importId}`,
    opts,
  );
}

/**
 * An account's Export Runs, newest first: the logged-in one in full, or as
 * the owner the one named, each without what the run asked for.
 */
export function listAccountExports(
  params: AccountRunListParams,
  opts?: RequestOptions,
  accountId?: number,
): Promise<Schema["AccountExportRuns"]> {
  return apiClient.get<Schema["AccountExportRuns"]>(
    withQuery(`${accountBase(accountId)}/exports`, query(params)),
    opts,
  );
}

/** Which page of an Audit Trail to read. Absent values are left off the URL. */
export type AuditTrailParams = { limit?: number; offset?: number };

/** Every account's Audit Trail, newest first. The owner's alone. */
export function listAuditTrail(
  params: AuditTrailParams,
  opts?: RequestOptions,
): Promise<Schema["Page_AuditEntry"]> {
  return apiClient.get<Schema["Page_AuditEntry"]>(
    withQuery("/v1/audit-trail", query(params)),
    opts,
  );
}

/** One account's Audit Trail, newest first: the logged-in one, or as the owner the one named. */
export function listAccountAuditTrail(
  params: AuditTrailParams,
  opts?: RequestOptions,
  accountId?: number,
): Promise<Schema["Page_AuditEntry"]> {
  return apiClient.get<Schema["Page_AuditEntry"]>(
    withQuery(`${accountBase(accountId)}/audit-trail`, query(params)),
    opts,
  );
}

/** Destroy the logged-in account's messages and attachments. Contacts and the login survive. */
export function deleteAllMessages(
  body: Schema["DeleteMessagesRequest"],
): Promise<Schema["DeleteMessagesResponse"]> {
  return apiClient.delete<Schema["DeleteMessagesResponse"]>(`${ownAccountPath()}/messages`, body);
}

// ── API tokens ──────────────────────────────────────────────────────────────
//
// An account's tokens live under its own row. The account makes, renames,
// lists and revokes them; the owner lists and revokes them too, so it can end
// a leaked one, and its list carries no `token_hint`.

/** An account's API tokens, every page: the logged-in one's, or as the owner the one named. */
export function listApiTokens(
  opts?: RequestOptions,
  accountId?: number,
): Promise<Schema["ApiToken"][]> {
  const path = `${accountBase(accountId)}/api-tokens`;
  return readEveryPage<Schema["Page_ApiToken"]>(path, opts);
}

export function createApiToken(
  body: Schema["CreateApiTokenRequest"],
): Promise<Schema["CreateApiTokenResponse"]> {
  return apiClient.post<Schema["CreateApiTokenResponse"]>(`${ownAccountPath()}/api-tokens`, body);
}

export function renameApiToken(
  id: number,
  body: Schema["UpdateApiTokenRequest"],
): Promise<Schema["ApiToken"]> {
  return apiClient.patch<Schema["ApiToken"]>(`${ownAccountPath()}/api-tokens/${id}`, body);
}

/** Revoke one API token: the logged-in account's, or as the owner one of the account named. */
export function deleteApiToken(id: number, accountId?: number): Promise<void> {
  return apiClient.delete<void>(`${accountBase(accountId)}/api-tokens/${id}`);
}

// ── Assets ──────────────────────────────────────────────────────────────────

/**
 * Download an attachment by its content hash and return a temporary blob URL:
 * the original bytes, or with `preview` the preview the server holds of them.
 * The caller must call `URL.revokeObjectURL` when the URL is no longer needed.
 */
export async function fetchAssetObjectUrl(
  sha256: string,
  source: string,
  { preview = false, signal }: { preview?: boolean; signal?: AbortSignal } = {},
): Promise<string> {
  const path = preview ? buildAssetPreviewPath(sha256, source) : buildAssetPath(sha256, source);
  const headers: Record<string, string> = {};
  const token = getToken();
  if (token) {
    headers.Authorization = `Bearer ${token}`;
  }
  // Attachment bytes are a blob, not JSON, so this is the one route that goes
  // around `apiClient` and calls `fetch` itself.
  const res = await fetch(`${getBaseUrl()}${path}`, { method: "GET", headers, signal });
  if (!res.ok) {
    const text = await res.text();
    throw problemFromBody(res.status, text);
  }
  const blob = await res.blob();
  return URL.createObjectURL(blob);
}

// ── Conversations ───────────────────────────────────────────────────────────

/** Filters the conversation list accepts. Absent values are left off the URL. */
export type ConversationListParams = {
  q?: string;
  limit?: number;
  offset?: number;
  /** `sort=-field,field`: `date` or `messages`, a leading `-` for descending. */
  sort?: string;
};

export function listConversations(
  params: ConversationListParams,
  opts?: RequestOptions,
): Promise<Schema["Page_ConversationSummary"]> {
  return apiClient.get<Schema["Page_ConversationSummary"]>(
    withQuery("/v1/conversations", query(params)),
    opts,
  );
}

export function getConversation(
  conversationId: number,
  opts?: RequestOptions,
): Promise<Schema["ConversationSummary"]> {
  return apiClient.get<Schema["ConversationSummary"]>(`/v1/conversations/${conversationId}`, opts);
}

/**
 * Paging for `GET /v1/conversations/{id}/messages`. Opening a conversation
 * takes no filter: a year inside one is the search `in:#id date:YYYY`.
 *
 * The page starts at `offset`, or beside one message: `around` puts it in the
 * middle, `before` and `after` read the page just before or after it in the
 * page's order. Send at most one of the four; the answer's `offset` says
 * where the page sits.
 */
export type ConversationMessagesParams = {
  offset?: number;
  limit?: number;
  /** `date` (oldest first, the default) or `-date`. */
  sort?: "date" | "-date";
  around?: number;
  before?: number;
  after?: number;
};

export function listConversationMessages(
  conversationId: number,
  params: ConversationMessagesParams,
  opts?: RequestOptions,
): Promise<Schema["Page_Message"]> {
  return apiClient.get<Schema["Page_Message"]>(
    withQuery(`/v1/conversations/${conversationId}/messages`, query(params)),
    opts,
  );
}

/** Query for `GET /v1/messages`: the search language's Messages list, paged. */
export type MessagesListParams = {
  q?: string;
  offset?: number;
  limit?: number;
  /** `date` (oldest first, the default) or `-date`. */
  sort?: "date" | "-date";
};

/**
 * One row per message matching `q`, across every conversation the account
 * has. A read route, not Export: the thread's find box uses it with `in:#id`.
 */
export function listMessages(
  params: MessagesListParams,
  opts?: RequestOptions,
): Promise<Schema["Page_Message"]> {
  return apiClient.get<Schema["Page_Message"]>(withQuery("/v1/messages", query(params)), opts);
}

/** Every source a conversation's messages came from. */
export function getConversationSources(
  conversationId: number,
  opts?: RequestOptions,
): Promise<Schema["ConversationSource"][]> {
  const path = `/v1/conversations/${conversationId}/sources`;
  return readEveryPage<Schema["Page_ConversationSource"]>(path, opts);
}

/** Put a conversation in the trash. Idempotent: trashing an already-trashed one still answers. */
export function trashConversation(conversationId: number): Promise<void> {
  return apiClient.post<void>(`/v1/conversations/${conversationId}/trash`, {});
}

/** Take a conversation out of the trash. Idempotent: restoring one that was not trashed still answers. */
export function restoreConversation(conversationId: number): Promise<void> {
  return apiClient.post<void>(`/v1/conversations/${conversationId}/restore`, {});
}

/**
 * Permanently delete a trashed conversation: the conversation, its messages,
 * and any attachment file no other message still uses. The server answers 409
 * for a conversation that is not in the trash — trash is the only door.
 */
export function deleteConversation(conversationId: number): Promise<void> {
  return apiClient.delete<void>(`/v1/conversations/${conversationId}`);
}

// ── Trash ───────────────────────────────────────────────────────────────────

/**
 * Empty the trash: every trashed conversation is deleted for good, and every
 * trashed contact loses its name and details and becomes Unknown, its
 * conversations untouched.
 */
export function emptyTrash(): Promise<void> {
  return apiClient.delete<void>("/v1/trash");
}

// ── Contacts ────────────────────────────────────────────────────────────────

export type ContactListParams = { q?: string; limit?: number; offset?: number };

export function listContacts(
  params: ContactListParams,
  opts?: RequestOptions,
): Promise<Schema["Page_ContactSummary"]> {
  return apiClient.get<Schema["Page_ContactSummary"]>(
    withQuery("/v1/contacts", query(params)),
    opts,
  );
}

export function getContact(
  contactId: string | number,
  opts?: RequestOptions,
): Promise<Schema["Contact"]> {
  return apiClient.get<Schema["Contact"]>(
    `/v1/contacts/${encodeURIComponent(String(contactId))}`,
    opts,
  );
}

/**
 * Change one thing about a contact: its preferred name, or one identity added,
 * updated, or removed. The server answers with the contact as it now stands.
 */
export function updateContact(
  contactId: string | number,
  body: Schema["UpdateContactRequest"],
): Promise<Schema["Contact"]> {
  return apiClient.patch<Schema["Contact"]>(
    `/v1/contacts/${encodeURIComponent(String(contactId))}`,
    body,
  );
}

export function getContactSummaries(
  body: Schema["ListContactSummariesRequest"],
  opts?: RequestOptions,
): Promise<Schema["Page_ContactSelectionSummary"]> {
  return apiClient.post<Schema["Page_ContactSelectionSummary"]>(
    "/v1/contacts/summaries",
    body,
    opts,
  );
}

/** Which of these identifiers the account has no contact for. */
export function unmatchedIdentities(
  body: Schema["ListUnmatchedIdentitiesRequest"],
  opts?: RequestOptions,
): Promise<Schema["Page_String"]> {
  return apiClient.post<Schema["Page_String"]>("/v1/contacts/unmatched-identities", body, opts);
}

/** How a load applies the address book: `append` removes nothing, `edit` makes each contact match its rows. */
export type AddressBookLoadMode = Schema["LoadMode"];

/**
 * Load an address book: the file's text is the body, as `text/csv`, and
 * `mode` says how it is applied. A file that breaks a rule is refused whole,
 * and the error names each bad row.
 */
export function loadAddressBook(
  content: string,
  mode: AddressBookLoadMode,
): Promise<Schema["CreateContactsResponse"]> {
  return apiClient.postRaw<Schema["CreateContactsResponse"]>(
    withQuery("/v1/contacts", query({ mode })),
    content,
    "text/csv",
  );
}

/**
 * The address book as CSV text, for the contacts a search matches, the
 * checked ones, or every contact when the body names neither.
 */
export function exportAddressBook(body: Schema["GetAddressBookRequest"]): Promise<string> {
  return apiClient.postText("/v1/contacts/address-book", body);
}

/** Put a contact in the trash. Idempotent: trashing an already-trashed one still answers. */
export function trashContact(contactId: string | number): Promise<void> {
  return apiClient.post<void>(`/v1/contacts/${encodeURIComponent(String(contactId))}/trash`, {});
}

/** Take a contact out of the trash. Idempotent: restoring one that was not trashed still answers. */
export function restoreContact(contactId: string | number): Promise<void> {
  return apiClient.post<void>(`/v1/contacts/${encodeURIComponent(String(contactId))}/restore`, {});
}

/**
 * Delete a trashed contact the way a phone's Delete Contact does: the name
 * and details go, the contact becomes Unknown again and leaves the trash, and
 * its conversations stay, showing the handle. The server answers 409 for a
 * contact that is not in the trash.
 */
export function deleteContact(contactId: string | number): Promise<void> {
  return apiClient.delete<void>(`/v1/contacts/${encodeURIComponent(String(contactId))}`);
}

// ── Contact Groups ──────────────────────────────────────────────────────────
//
// A Contact Group is addressed by its id. Screens hold names; the lookup from
// a name to an id lives in `nameCollection.ts`, not here.

/** Every Contact Group of the logged-in account. */
export function listContactGroups(opts?: RequestOptions): Promise<Schema["NamedSet"][]> {
  return readEveryPage<Schema["Page_NamedSet"]>("/v1/contact-groups", opts);
}

export function createContactGroup(
  body: Schema["CreateNamedSetRequest"],
  opts?: RequestOptions,
): Promise<Schema["NamedSet"]> {
  return apiClient.post<Schema["NamedSet"]>("/v1/contact-groups", body, opts);
}

export function updateContactGroup(
  id: number,
  body: Schema["UpdateNamedSetRequest"],
  opts?: RequestOptions,
): Promise<Schema["NamedSet"]> {
  return apiClient.patch<Schema["NamedSet"]>(`/v1/contact-groups/${id}`, body, opts);
}

export function deleteContactGroup(id: number, opts?: RequestOptions): Promise<void> {
  return apiClient.delete<void>(`/v1/contact-groups/${id}`, undefined, opts);
}

export function listContactGroupMembers(
  id: number,
  opts?: RequestOptions,
): Promise<Schema["Page_i64"]> {
  return apiClient.get<Schema["Page_i64"]>(`/v1/contact-groups/${id}/members`, opts);
}

export function updateContactGroupMembers(
  id: number,
  body: Schema["UpdateMembersRequest"],
  opts?: RequestOptions,
): Promise<Schema["UpdateMembersResponse"]> {
  return apiClient.patch<Schema["UpdateMembersResponse"]>(
    `/v1/contact-groups/${id}/members`,
    body,
    opts,
  );
}

// ── Message Tags ────────────────────────────────────────────────────────────

/** Every Message Tag of the logged-in account. */
export function listMessageTags(opts?: RequestOptions): Promise<Schema["NamedSet"][]> {
  return readEveryPage<Schema["Page_NamedSet"]>("/v1/message-tags", opts);
}

export function createMessageTag(
  body: Schema["CreateNamedSetRequest"],
  opts?: RequestOptions,
): Promise<Schema["NamedSet"]> {
  return apiClient.post<Schema["NamedSet"]>("/v1/message-tags", body, opts);
}

export function updateMessageTag(
  id: number,
  body: Schema["UpdateNamedSetRequest"],
  opts?: RequestOptions,
): Promise<Schema["NamedSet"]> {
  return apiClient.patch<Schema["NamedSet"]>(`/v1/message-tags/${id}`, body, opts);
}

export function deleteMessageTag(id: number, opts?: RequestOptions): Promise<void> {
  return apiClient.delete<void>(`/v1/message-tags/${id}`, undefined, opts);
}

export function listMessageTagMembers(
  id: number,
  opts?: RequestOptions,
): Promise<Schema["Page_i64"]> {
  return apiClient.get<Schema["Page_i64"]>(`/v1/message-tags/${id}/members`, opts);
}

export function updateMessageTagMembers(
  id: number,
  body: Schema["UpdateMembersRequest"],
  opts?: RequestOptions,
): Promise<Schema["UpdateMembersResponse"]> {
  return apiClient.patch<Schema["UpdateMembersResponse"]>(
    `/v1/message-tags/${id}/members`,
    body,
    opts,
  );
}

// ── Saved Searches ──────────────────────────────────────────────────────────

/** Every Saved Search of the logged-in account. */
export function listSavedSearches(opts?: RequestOptions): Promise<Schema["SavedSearch"][]> {
  return readEveryPage<Schema["Page_SavedSearch"]>("/v1/saved-searches", opts);
}

export function createSavedSearch(
  body: Schema["CreateSavedSearchRequest"],
): Promise<Schema["SavedSearch"]> {
  return apiClient.post<Schema["SavedSearch"]>("/v1/saved-searches", body);
}

export function updateSavedSearch(
  id: number,
  body: Schema["UpdateSavedSearchRequest"],
): Promise<Schema["SavedSearch"]> {
  return apiClient.patch<Schema["SavedSearch"]>(`/v1/saved-searches/${id}`, body);
}

export function deleteSavedSearch(id: number): Promise<void> {
  return apiClient.delete<void>(`/v1/saved-searches/${id}`);
}

// ── Search ──────────────────────────────────────────────────────────────────

/** The lists whose search words the server describes, one path each. */
export type SearchFieldList = "contacts" | "conversations";

/** Every word the search language accepts on one list. */
export function listSearchFields(
  list: SearchFieldList,
  opts?: RequestOptions,
): Promise<Schema["FieldDoc"][]> {
  const path = `/v1/search-fields/${list}`;
  return readEveryPage<Schema["Page_FieldDoc"]>(path, opts);
}

// ── Import Runs ─────────────────────────────────────────────────────────────

/** The account's Import Runs, newest first, narrowed to one status when given. */
export type ImportListParams = {
  status?: Schema["ImportStatus"];
  limit?: number;
  offset?: number;
};

export function listImports(
  params: ImportListParams = {},
  opts?: RequestOptions,
): Promise<Schema["Page_ImportRun"]> {
  return apiClient.get<Schema["Page_ImportRun"]>(withQuery("/v1/imports", query(params)), opts);
}

export function getImport(id: number, opts?: RequestOptions): Promise<Schema["ImportRun"]> {
  return apiClient.get<Schema["ImportRun"]>(`/v1/imports/${id}`, opts);
}

export function createImport(
  body: Schema["CreateImportRequest"],
): Promise<Schema["CreateImportResponse"]> {
  return apiClient.post<Schema["CreateImportResponse"]>("/v1/imports", body);
}

/** Move a live Import Run to another stage; the run comes back. */
export function setImportStage(
  id: number,
  body: Schema["UpdateImportRequest"],
): Promise<Schema["ImportRun"]> {
  return apiClient.patch<Schema["ImportRun"]>(`/v1/imports/${id}`, body);
}

export function completeImport(
  id: number,
  body: Schema["CompleteImportRequest"],
): Promise<Schema["ImportRun"]> {
  return apiClient.post<Schema["ImportRun"]>(`/v1/imports/${id}/complete`, body);
}

export function discardImport(id: number): Promise<Schema["ImportRun"]> {
  return apiClient.post<Schema["ImportRun"]>(`/v1/imports/${id}/discard`, {});
}

/** Which page of an Import Run's contacts to read. Absent values are left off the URL. */
export type ImportContactsParams = { limit?: number; offset?: number };

/** One page of the contacts an Import Run created or changed. */
export function getImportContacts(
  id: number,
  params: ImportContactsParams,
  opts?: RequestOptions,
): Promise<Schema["Page_ImportContact"]> {
  return apiClient.get<Schema["Page_ImportContact"]>(
    withQuery(`/v1/imports/${id}/contacts`, query(params)),
    opts,
  );
}

// ── Export Runs ─────────────────────────────────────────────────────────────
//
// The desktop app pages a run's messages from its Rust side (`message-crate-pull`),
// so `GET /v1/exports/{id}/messages` has no function here.

export function getExport(id: number, opts?: RequestOptions): Promise<Schema["ExportRun"]> {
  return apiClient.get<Schema["ExportRun"]>(`/v1/exports/${id}`, opts);
}

/** Record an Export Run; the server answers `201` with the run and its counts. */
export function createExport(body: Schema["CreateExportRequest"]): Promise<Schema["ExportRun"]> {
  return apiClient.post<Schema["ExportRun"]>("/v1/exports", body);
}

export function completeExport(id: number): Promise<Schema["ExportRun"]> {
  return apiClient.post<Schema["ExportRun"]>(`/v1/exports/${id}/complete`, {});
}

export function cancelExport(id: number): Promise<Schema["ExportRun"]> {
  return apiClient.post<Schema["ExportRun"]>(`/v1/exports/${id}/cancel`, {});
}

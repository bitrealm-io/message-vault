/**
 * Every vault route the web app calls, one named function each.
 *
 * This is the only module that knows a vault URL. Screens call
 * `listConversations` rather than writing `/v1/conversations?…`, so renaming a
 * route is a change here and nowhere else, and no test has to match on a path.
 *
 * Request and response types come from `vaultApi.types.ts`, which is generated
 * from `docs/src/assets/openapi.json` — the document a vault-side test pins to
 * the running server. Regenerate with `npm run gen:api`; `scripts/check-pr.sh`
 * fails when the checked-in file is out of date.
 *
 * These functions only talk to the vault. Caching, request deduplication, and
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
import { buildAssetPath } from "./assetUrl";
import type { components } from "./vaultApi.types";

type Schema = components["schemas"];

/** Options every read accepts, so a caller can cancel an in-flight request. */
export type VaultRequestOptions = ApiRequestOptions;

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
 * The vault has no `/v1/account` singleton: an account reads and writes its
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

/** Log in. The Session is a singleton, so the vault answers `201` with `Location: /v1/session`. */
export function login(
  body: Schema["CreateSessionRequest"],
): Promise<Schema["CreateSessionResponse"]> {
  return apiClient.post<Schema["CreateSessionResponse"]>("/v1/session", body);
}

/** The Session the bearer token names: its account, username, and import sources. */
export function getSession(opts?: VaultRequestOptions): Promise<Schema["Session"]> {
  return apiClient.get<Schema["Session"]>("/v1/session", opts);
}

/** Log out: end the Session. The vault answers `204`. */
export function logout(opts?: VaultRequestOptions): Promise<void> {
  return apiClient.delete<void>("/v1/session", undefined, opts);
}

// ── The vault itself ────────────────────────────────────────────────────────

/**
 * What state this vault is in, for the screen a logged-out visitor sees.
 *
 * The vault reports one value rather than the facts behind it, so the rule
 * joining "does an owner exist" to "is registration open" is stated once, on
 * the server. See `docs/adr/0008-the-vault-owner-holds-no-messages.md`.
 */
export function getVaultState(opts?: VaultRequestOptions): Promise<Schema["Vault"]> {
  return apiClient.get<Schema["Vault"]>("/v1/vault", opts);
}

/** Claim an unclaimed vault by creating its owner. Returns their session. */
export function claimVault(
  body: Schema["ClaimVaultRequest"],
): Promise<Schema["CreateSessionResponse"]> {
  return apiClient.post<Schema["CreateSessionResponse"]>("/v1/vault/claim", body);
}

// ── The accounts collection ─────────────────────────────────────────────────
//
// One collection for the vault owner and for each account: the owner reaches
// every row, an account reaches its own. The functions Owner Home calls take
// the account id; the ones Settings calls address the logged-in
// account through `ownAccountPath`.

/** The accounts of this vault, for the owner: the owner's own first, then the rest by username. */
export function listAccounts(opts?: VaultRequestOptions): Promise<Schema["Page_Account"]> {
  return apiClient.get<Schema["Page_Account"]>("/v1/accounts", opts);
}

/**
 * Create an account.
 *
 * Logged out, on an open vault, this is registration: the vault opens a
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
export function getAccount(
  accountId: number,
  opts?: VaultRequestOptions,
): Promise<Schema["Account"]> {
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

/** Settings that belong to the whole vault. */
export function getVaultSettings(opts?: VaultRequestOptions): Promise<Schema["VaultSettings"]> {
  return apiClient.get<Schema["VaultSettings"]>("/v1/vault/settings", opts);
}

/**
 * What the whole vault holds, summed over every account: message,
 * conversation, contact and attachment counts, and attachment bytes. The
 * owner's, and counts only (`docs/adr/0008-the-vault-owner-holds-no-messages.md`).
 */
export function getVaultStorage(opts?: VaultRequestOptions): Promise<Schema["VaultStorage"]> {
  return apiClient.get<Schema["VaultStorage"]>("/v1/vault/storage", opts);
}

/** Change the vault's settings. Omitted fields are left alone. */
export function updateVaultSettings(
  body: Schema["UpdateVaultSettingsRequest"],
): Promise<Schema["VaultSettings"]> {
  return apiClient.patch<Schema["VaultSettings"]>("/v1/vault/settings", body);
}

// ── The logged-in account's own row ─────────────────────────────────────────

/** The logged-in account: profile, flags, and how much it holds. */
export function getAccountProfile(opts?: VaultRequestOptions): Promise<Schema["Account"]> {
  return apiClient.get<Schema["Account"]>(ownAccountPath(), opts);
}

/** Change the logged-in account's display name, time zone or handles. */
export function updateAccountProfile(
  body: Schema["UpdateAccountRequest"],
): Promise<Schema["Account"]> {
  return apiClient.patch<Schema["Account"]>(ownAccountPath(), body);
}

/** Change the logged-in account's own password. The vault answers a rotated session token. */
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
  opts?: VaultRequestOptions,
  accountId?: number,
): Promise<Schema["AccountStorage"]> {
  return apiClient.get<Schema["AccountStorage"]>(`${accountBase(accountId)}/storage`, opts);
}

// An account's import and export history, for its Storage screen. These are
// not `/v1/imports` and `/v1/exports`: those are the pipelines' own routes and
// ask for a permission, which the vault owner's session never carries.

function accountBase(accountId?: number): string {
  return accountId === undefined ? ownAccountPath() : accountPath(accountId);
}

/** An account's identities with the messages held at each: the logged-in one, or as the owner the one named. */
export function listAccountIdentities(
  opts?: VaultRequestOptions,
  accountId?: number,
): Promise<Schema["Page_Identity"]> {
  return apiClient.get<Schema["Page_Identity"]>(`${accountBase(accountId)}/identities`, opts);
}

/** An account's Import Runs, newest first: the logged-in one, or as the owner the one named. */
export function listAccountImports(
  opts?: VaultRequestOptions,
  accountId?: number,
): Promise<Schema["Page_ImportSummary"]> {
  return apiClient.get<Schema["Page_ImportSummary"]>(`${accountBase(accountId)}/imports`, opts);
}

/** One of an account's Import Runs, with its counts, timings and issues. */
export function getAccountImport(
  importId: number,
  opts?: VaultRequestOptions,
  accountId?: number,
): Promise<Schema["ImportRun"]> {
  return apiClient.get<Schema["ImportRun"]>(`${accountBase(accountId)}/imports/${importId}`, opts);
}

/** An account's Export Runs, newest first: the logged-in one, or as the owner the one named. */
export function listAccountExports(
  opts?: VaultRequestOptions,
  accountId?: number,
): Promise<Schema["Page_ExportRun"]> {
  return apiClient.get<Schema["Page_ExportRun"]>(`${accountBase(accountId)}/exports`, opts);
}

/** Destroy the logged-in account's messages and attachments. Contacts and the login survive. */
export function deleteAllMessages(
  body: Schema["DeleteMessagesRequest"],
): Promise<Schema["DeleteMessagesResponse"]> {
  return apiClient.delete<Schema["DeleteMessagesResponse"]>(`${ownAccountPath()}/messages`, body);
}

// ── API tokens ──────────────────────────────────────────────────────────────
//
// An account's tokens live under its own row, and nobody else's session
// reaches them.

export function listApiTokens(opts?: VaultRequestOptions): Promise<Schema["Page_ApiToken"]> {
  return apiClient.get<Schema["Page_ApiToken"]>(`${ownAccountPath()}/api-tokens`, opts);
}

export function createApiToken(
  body: Schema["CreateApiTokenRequest"],
): Promise<Schema["CreateApiTokenResponse"]> {
  return apiClient.post<Schema["CreateApiTokenResponse"]>(`${ownAccountPath()}/api-tokens`, body);
}

export function renameApiToken(
  id: number,
  body: Schema["UpdateApiTokenRequest"],
): Promise<Schema["UpdateApiTokenResponse"]> {
  return apiClient.patch<Schema["UpdateApiTokenResponse"]>(
    `${ownAccountPath()}/api-tokens/${id}`,
    body,
  );
}

export function deleteApiToken(id: number): Promise<void> {
  return apiClient.delete<void>(`${ownAccountPath()}/api-tokens/${id}`);
}

// ── Assets ──────────────────────────────────────────────────────────────────

/**
 * Download an attachment by its content hash and return a temporary blob URL.
 * The caller must call `URL.revokeObjectURL` when the URL is no longer needed.
 */
export async function fetchAssetObjectUrl(
  sha256: string,
  source: string,
  signal?: AbortSignal,
): Promise<string> {
  const path = buildAssetPath(sha256, source);
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
  count_only?: boolean;
};

export function listConversations(
  params: ConversationListParams,
  opts?: VaultRequestOptions,
): Promise<Schema["Page_ConversationSummary"]> {
  return apiClient.get<Schema["Page_ConversationSummary"]>(
    withQuery("/v1/conversations", query(params)),
    opts,
  );
}

export function getConversation(
  conversationId: number,
  opts?: VaultRequestOptions,
): Promise<Schema["ConversationSummary"]> {
  return apiClient.get<Schema["ConversationSummary"]>(`/v1/conversations/${conversationId}`, opts);
}

/** Paging for `GET /v1/conversations/{id}/messages`. Opening a conversation
 * takes no filter: a year inside one is the search `in:#id date:YYYY`. */
export type ConversationMessagesParams = {
  offset?: number;
  limit?: number;
};

export function listConversationMessages(
  conversationId: number,
  params: ConversationMessagesParams,
  opts?: VaultRequestOptions,
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
};

/**
 * One row per message matching `q`, across every conversation the account
 * has. A read route, not Export: the thread's find box uses it with `in:#id`.
 */
export function listMessages(
  params: MessagesListParams,
  opts?: VaultRequestOptions,
): Promise<Schema["Page_Message"]> {
  return apiClient.get<Schema["Page_Message"]>(withQuery("/v1/messages", query(params)), opts);
}

export function getConversationSources(
  conversationId: number,
  opts?: VaultRequestOptions,
): Promise<Schema["Page_ConversationSource"]> {
  return apiClient.get<Schema["Page_ConversationSource"]>(
    `/v1/conversations/${conversationId}/sources`,
    opts,
  );
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
 * and any attachment file no other message still uses. The vault answers 409
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
  opts?: VaultRequestOptions,
): Promise<Schema["Page_ContactSummary"]> {
  return apiClient.get<Schema["Page_ContactSummary"]>(
    withQuery("/v1/contacts", query(params)),
    opts,
  );
}

export function getContact(
  contactId: string | number,
  opts?: VaultRequestOptions,
): Promise<Schema["Contact"]> {
  return apiClient.get<Schema["Contact"]>(
    `/v1/contacts/${encodeURIComponent(String(contactId))}`,
    opts,
  );
}

/**
 * Change one thing about a contact: its preferred name, or one handle added,
 * updated, or removed. The vault answers with the contact as it now stands.
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
  body: Schema["SummarizeContactsRequest"],
  opts?: VaultRequestOptions,
): Promise<Schema["Page_ContactSelectionSummary"]> {
  return apiClient.post<Schema["Page_ContactSelectionSummary"]>(
    "/v1/contacts/summaries",
    body,
    opts,
  );
}

/** Which of these identifiers the account has no contact for. */
export function unmatchedIdentities(
  body: Schema["FindUnmatchedIdentitiesRequest"],
): Promise<Schema["Page_String"]> {
  return apiClient.post<Schema["Page_String"]>("/v1/contacts/unmatched-identities", body);
}

/** The media type an address book file is sent as, from its name; null when it is neither. */
export function addressBookContentType(fileName: string): "text/vcard" | "text/csv" | null {
  const lower = fileName.trim().toLowerCase();
  if (lower.endsWith(".vcf") || lower.endsWith(".vcard")) return "text/vcard";
  if (lower.endsWith(".csv")) return "text/csv";
  return null;
}

/**
 * Load an address book: the file's text is the body, and its media type says
 * whether it is a vCard file or a vCard CSV export.
 */
export function loadAddressBook(
  content: string,
  contentType: "text/vcard" | "text/csv",
): Promise<Schema["CreateContactsResponse"]> {
  return apiClient.postRaw<Schema["CreateContactsResponse"]>("/v1/contacts", content, contentType);
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
 * its conversations stay, showing the handle. The vault answers 409 for a
 * contact that is not in the trash.
 */
export function deleteContact(contactId: string | number): Promise<void> {
  return apiClient.delete<void>(`/v1/contacts/${encodeURIComponent(String(contactId))}`);
}

// ── Contact Groups ──────────────────────────────────────────────────────────
//
// A Contact Group is addressed by its id. Screens hold names; the lookup from
// a name to an id lives in `nameCollection.ts`, not here.

export function listContactGroups(opts?: VaultRequestOptions): Promise<Schema["Page_NamedSet"]> {
  return apiClient.get<Schema["Page_NamedSet"]>("/v1/contact-groups", opts);
}

export function createContactGroup(
  body: Schema["NamedSetRequest"],
  opts?: VaultRequestOptions,
): Promise<Schema["NamedSet"]> {
  return apiClient.post<Schema["NamedSet"]>("/v1/contact-groups", body, opts);
}

export function updateContactGroup(
  id: number,
  body: Schema["NamedSetRequest"],
  opts?: VaultRequestOptions,
): Promise<Schema["NamedSet"]> {
  return apiClient.patch<Schema["NamedSet"]>(`/v1/contact-groups/${id}`, body, opts);
}

export function deleteContactGroup(id: number, opts?: VaultRequestOptions): Promise<void> {
  return apiClient.delete<void>(`/v1/contact-groups/${id}`, undefined, opts);
}

export function listContactGroupMembers(
  id: number,
  opts?: VaultRequestOptions,
): Promise<Schema["Page_i64"]> {
  return apiClient.get<Schema["Page_i64"]>(`/v1/contact-groups/${id}/members`, opts);
}

export function updateContactGroupMembers(
  id: number,
  body: Schema["UpdateMembersRequest"],
  opts?: VaultRequestOptions,
): Promise<Schema["UpdateMembersResponse"]> {
  return apiClient.patch<Schema["UpdateMembersResponse"]>(
    `/v1/contact-groups/${id}/members`,
    body,
    opts,
  );
}

// ── Message Tags ────────────────────────────────────────────────────────────

export function listMessageTags(opts?: VaultRequestOptions): Promise<Schema["Page_NamedSet"]> {
  return apiClient.get<Schema["Page_NamedSet"]>("/v1/message-tags", opts);
}

export function createMessageTag(
  body: Schema["NamedSetRequest"],
  opts?: VaultRequestOptions,
): Promise<Schema["NamedSet"]> {
  return apiClient.post<Schema["NamedSet"]>("/v1/message-tags", body, opts);
}

export function updateMessageTag(
  id: number,
  body: Schema["NamedSetRequest"],
  opts?: VaultRequestOptions,
): Promise<Schema["NamedSet"]> {
  return apiClient.patch<Schema["NamedSet"]>(`/v1/message-tags/${id}`, body, opts);
}

export function deleteMessageTag(id: number, opts?: VaultRequestOptions): Promise<void> {
  return apiClient.delete<void>(`/v1/message-tags/${id}`, undefined, opts);
}

export function listMessageTagMembers(
  id: number,
  opts?: VaultRequestOptions,
): Promise<Schema["Page_i64"]> {
  return apiClient.get<Schema["Page_i64"]>(`/v1/message-tags/${id}/members`, opts);
}

export function updateMessageTagMembers(
  id: number,
  body: Schema["UpdateMembersRequest"],
  opts?: VaultRequestOptions,
): Promise<Schema["UpdateMembersResponse"]> {
  return apiClient.patch<Schema["UpdateMembersResponse"]>(
    `/v1/message-tags/${id}/members`,
    body,
    opts,
  );
}

// ── Saved Searches ──────────────────────────────────────────────────────────

export function listSavedSearches(opts?: VaultRequestOptions): Promise<Schema["Page_SavedSearch"]> {
  return apiClient.get<Schema["Page_SavedSearch"]>("/v1/saved-searches", opts);
}

export function createSavedSearch(
  body: Schema["SavedSearchRequest"],
): Promise<Schema["SavedSearch"]> {
  return apiClient.post<Schema["SavedSearch"]>("/v1/saved-searches", body);
}

export function updateSavedSearch(
  id: number,
  body: Schema["SavedSearchRequest"],
): Promise<Schema["SavedSearch"]> {
  return apiClient.patch<Schema["SavedSearch"]>(`/v1/saved-searches/${id}`, body);
}

export function deleteSavedSearch(id: number): Promise<void> {
  return apiClient.delete<void>(`/v1/saved-searches/${id}`);
}

// ── Search ──────────────────────────────────────────────────────────────────

/** The lists whose search words the vault describes, one path each. */
export type SearchFieldList = "contacts" | "conversations";

/** The words the search language accepts on one list. */
export function listSearchFields(
  list: SearchFieldList,
  opts?: VaultRequestOptions,
): Promise<Schema["Page_FieldDoc"]> {
  return apiClient.get<Schema["Page_FieldDoc"]>(`/v1/search-fields/${list}`, opts);
}

// ── Import Runs ─────────────────────────────────────────────────────────────

/** The account's Import Runs, newest first, narrowed to one status when given. */
export type ImportListParams = {
  status?: "running" | "completed" | "completed_with_issues" | "failed" | "cancelled";
  limit?: number;
  offset?: number;
};

export function listImports(
  params: ImportListParams = {},
  opts?: VaultRequestOptions,
): Promise<Schema["Page_ImportSummary"]> {
  return apiClient.get<Schema["Page_ImportSummary"]>(withQuery("/v1/imports", query(params)), opts);
}

export function getImport(id: number, opts?: VaultRequestOptions): Promise<Schema["ImportRun"]> {
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
): Promise<Schema["CompleteImportResponse"]> {
  return apiClient.post<Schema["CompleteImportResponse"]>(`/v1/imports/${id}/complete`, body);
}

export function discardImport(id: number): Promise<Schema["DiscardImportResponse"]> {
  return apiClient.post<Schema["DiscardImportResponse"]>(`/v1/imports/${id}/discard`, {});
}

export function getImportContacts(
  id: number,
  opts?: VaultRequestOptions,
): Promise<Schema["Page_ImportContact"]> {
  return apiClient.get<Schema["Page_ImportContact"]>(`/v1/imports/${id}/contacts`, opts);
}

// ── Export Runs ─────────────────────────────────────────────────────────────
//
// The desktop app pages a run's messages from its Rust side (`vault-pull`),
// so `GET /v1/exports/{id}/messages` has no function here.

export function getExport(id: number, opts?: VaultRequestOptions): Promise<Schema["ExportRun"]> {
  return apiClient.get<Schema["ExportRun"]>(`/v1/exports/${id}`, opts);
}

/** Record an Export Run; the vault answers `201` with the run and its counts. */
export function createExport(body: Schema["CreateExportRequest"]): Promise<Schema["ExportRun"]> {
  return apiClient.post<Schema["ExportRun"]>("/v1/exports", body);
}

export function completeExport(id: number): Promise<Schema["ExportRun"]> {
  return apiClient.post<Schema["ExportRun"]>(`/v1/exports/${id}/complete`, {});
}

export function cancelExport(id: number): Promise<Schema["ExportRun"]> {
  return apiClient.post<Schema["ExportRun"]>(`/v1/exports/${id}/cancel`, {});
}

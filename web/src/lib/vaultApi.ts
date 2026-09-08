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
  getBaseUrl,
  getToken,
  problemFromBody,
  VaultApiError,
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

// ── Auth ────────────────────────────────────────────────────────────────────

export function login(body: Schema["LoginRequest"]): Promise<Schema["AuthTokenResponse"]> {
  return apiClient.post<Schema["AuthTokenResponse"]>("/v1/auth/login", body);
}

export function register(body: Schema["RegisterRequest"]): Promise<Schema["AuthTokenResponse"]> {
  return apiClient.post<Schema["AuthTokenResponse"]>("/v1/auth/register", body);
}

export function checkAuth(opts?: VaultRequestOptions): Promise<Schema["AuthCheckResponse"]> {
  return apiClient.get<Schema["AuthCheckResponse"]>("/v1/auth/check", opts);
}

export function logout(opts?: VaultRequestOptions): Promise<void> {
  return apiClient.post<void>("/v1/auth/logout", {}, opts);
}

export function changePassword(
  body: Schema["ChangePasswordRequest"],
): Promise<Schema["ChangePasswordResponse"]> {
  return apiClient.put<Schema["ChangePasswordResponse"]>("/v1/account/password", body);
}

export function deleteAccount(body: Schema["DeleteAccountRequest"]): Promise<void> {
  return apiClient.delete<void>("/v1/account", body);
}

// ── The vault itself ────────────────────────────────────────────────────────

/**
 * What state this vault is in, for the screen a signed-out visitor sees.
 *
 * The vault reports one value rather than the facts behind it, so the rule
 * joining "does an owner exist" to "is registration open" is stated once, on
 * the server. See `docs/adr/0008-the-vault-owner-holds-no-messages.md`.
 */
export function getVaultState(opts?: VaultRequestOptions): Promise<Schema["VaultResponse"]> {
  return apiClient.get<Schema["VaultResponse"]>("/v1/vault", opts);
}

/** Claim an unclaimed vault by creating its owner. Returns their session. */
export function claimVault(
  body: Schema["ClaimVaultRequest"],
): Promise<Schema["AuthTokenResponse"]> {
  return apiClient.post<Schema["AuthTokenResponse"]>("/v1/vault/claim", body);
}

// ── The vault owner's account management ────────────────────────────────────

/** The accounts of this vault. The owner's own is not among them. */
export function listAccounts(opts?: VaultRequestOptions): Promise<Schema["ListAccountsResponse"]> {
  return apiClient.get<Schema["ListAccountsResponse"]>("/v1/owner/accounts", opts);
}

/** Create an account. Its holder must replace this password at first sign-in. */
export function createAccount(
  body: Schema["CreateAccountRequest"],
): Promise<Schema["ManagedAccount"]> {
  return apiClient.post<Schema["ManagedAccount"]>("/v1/owner/accounts", body);
}

/** Change an account's disabled flag or its import, export and delete grants. */
export function updateAccount(
  accountId: string,
  body: Schema["PatchAccountRequest"],
): Promise<Schema["ManagedAccount"]> {
  return apiClient.patch<Schema["ManagedAccount"]>(
    `/v1/owner/accounts/${encodeURIComponent(accountId)}`,
    body,
  );
}

/** Set an account's password, ending its sessions. */
export function setAccountPassword(
  accountId: string,
  body: Schema["SetPasswordRequest"],
): Promise<void> {
  return apiClient.put<void>(`/v1/owner/accounts/${encodeURIComponent(accountId)}/password`, body);
}

/** Delete an account: its login, profile, contacts, and every message it owns. */
export function deleteAccountById(accountId: string): Promise<void> {
  return apiClient.delete<void>(`/v1/owner/accounts/${encodeURIComponent(accountId)}`);
}

/** Destroy one account's messages. The account, its contacts and login survive. */
export function deleteAccountMessages(accountId: string): Promise<unknown> {
  return apiClient.delete<unknown>(`/v1/owner/accounts/${encodeURIComponent(accountId)}/messages`);
}

/** Settings that belong to the whole vault. */
export function getVaultSettings(
  opts?: VaultRequestOptions,
): Promise<Schema["VaultSettingsResponse"]> {
  return apiClient.get<Schema["VaultSettingsResponse"]>("/v1/vault/settings", opts);
}

/** Change the vault's settings. Omitted fields are left alone. */
export function updateVaultSettings(
  body: Schema["PatchVaultSettingsRequest"],
): Promise<Schema["VaultSettingsResponse"]> {
  return apiClient.patch<Schema["VaultSettingsResponse"]>("/v1/vault/settings", body);
}

// ── Account ─────────────────────────────────────────────────────────────────

export function getAccountProfile(
  opts?: VaultRequestOptions,
): Promise<Schema["AccountProfileResponse"]> {
  return apiClient.get<Schema["AccountProfileResponse"]>("/v1/account/profile", opts);
}

export function updateAccountProfile(
  body: Schema["AccountProfileUpdateRequest"],
): Promise<Schema["AccountProfileResponse"]> {
  return apiClient.patch<Schema["AccountProfileResponse"]>("/v1/account/profile", body);
}

export function getAccountStorage(
  opts?: VaultRequestOptions,
): Promise<Schema["AccountStorageResponse"]> {
  return apiClient.get<Schema["AccountStorageResponse"]>("/v1/account/storage", opts);
}

export function deleteAllMessages(
  body: Schema["DeleteMessagesRequest"],
): Promise<Schema["DeleteMessagesResponse"]> {
  return apiClient.delete<Schema["DeleteMessagesResponse"]>("/v1/account/messages", body);
}

// ── API tokens ──────────────────────────────────────────────────────────────

export function listApiTokens(
  opts?: VaultRequestOptions,
): Promise<Schema["ListApiTokensResponse"]> {
  return apiClient.get<Schema["ListApiTokensResponse"]>("/v1/account/api-tokens", opts);
}

export function createApiToken(
  body: Schema["CreateApiTokenRequest"],
): Promise<Schema["CreateApiTokenResponse"]> {
  return apiClient.post<Schema["CreateApiTokenResponse"]>("/v1/account/api-tokens", body);
}

export function renameApiToken(
  id: string,
  body: Schema["RenameApiTokenRequest"],
): Promise<Schema["RenameApiTokenResponse"]> {
  return apiClient.patch<Schema["RenameApiTokenResponse"]>(
    `/v1/account/api-tokens/${encodeURIComponent(id)}`,
    body,
  );
}

export function deleteApiToken(id: string): Promise<void> {
  return apiClient.delete<void>(`/v1/account/api-tokens/${encodeURIComponent(id)}`);
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
  sort?: string;
  order?: string;
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

/** Filters `GET /v1/conversations/{id}/messages` accepts. */
export type ConversationMessagesParams = {
  offset?: number;
  limit?: number;
  year?: number;
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
): Promise<Schema["ConversationSourcesPage"]> {
  return apiClient.get<Schema["ConversationSourcesPage"]>(
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
): Promise<Schema["ContactDetail"]> {
  return apiClient.get<Schema["ContactDetail"]>(
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
  body: Schema["ContactMutationBody"],
): Promise<Schema["ContactDetail"]> {
  return apiClient.patch<Schema["ContactDetail"]>(
    `/v1/contacts/${encodeURIComponent(String(contactId))}`,
    body,
  );
}

export function getContactSummaries(
  body: Schema["ContactSummariesBody"],
  opts?: VaultRequestOptions,
): Promise<Schema["ContactSummariesPage"]> {
  return apiClient.post<Schema["ContactSummariesPage"]>("/v1/contacts/summaries", body, opts);
}

/** Which of these identifiers the account has no contact for. */
export function unmatchedHandles(
  body: Schema["UnmatchedHandlesBody"],
): Promise<Schema["UnmatchedHandlesResponse"]> {
  return apiClient.post<Schema["UnmatchedHandlesResponse"]>("/v1/contacts/unmatched-handles", body);
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
): Promise<Schema["AddressBookLoadResponse"]> {
  return apiClient.postRaw<Schema["AddressBookLoadResponse"]>("/v1/contacts", content, contentType);
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

export function listContactGroups(opts?: VaultRequestOptions): Promise<Schema["NamedSetList"]> {
  return apiClient.get<Schema["NamedSetList"]>("/v1/contact-groups", opts);
}

export function createContactGroup(
  body: Schema["NamedSetBody"],
  opts?: VaultRequestOptions,
): Promise<Schema["NamedSet"]> {
  return apiClient.post<Schema["NamedSet"]>("/v1/contact-groups", body, opts);
}

export function updateContactGroup(
  id: number,
  body: Schema["NamedSetBody"],
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
): Promise<Schema["MemberIdList"]> {
  return apiClient.get<Schema["MemberIdList"]>(`/v1/contact-groups/${id}/members`, opts);
}

export function updateContactGroupMembers(
  id: number,
  body: Schema["MembersPatch"],
  opts?: VaultRequestOptions,
): Promise<Schema["MembersChanged"]> {
  return apiClient.patch<Schema["MembersChanged"]>(`/v1/contact-groups/${id}/members`, body, opts);
}

// ── Message Tags ────────────────────────────────────────────────────────────

export function listMessageTags(opts?: VaultRequestOptions): Promise<Schema["NamedSetList"]> {
  return apiClient.get<Schema["NamedSetList"]>("/v1/message-tags", opts);
}

export function createMessageTag(
  body: Schema["NamedSetBody"],
  opts?: VaultRequestOptions,
): Promise<Schema["NamedSet"]> {
  return apiClient.post<Schema["NamedSet"]>("/v1/message-tags", body, opts);
}

export function updateMessageTag(
  id: number,
  body: Schema["NamedSetBody"],
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
): Promise<Schema["MemberIdList"]> {
  return apiClient.get<Schema["MemberIdList"]>(`/v1/message-tags/${id}/members`, opts);
}

export function updateMessageTagMembers(
  id: number,
  body: Schema["MembersPatch"],
  opts?: VaultRequestOptions,
): Promise<Schema["MembersChanged"]> {
  return apiClient.patch<Schema["MembersChanged"]>(`/v1/message-tags/${id}/members`, body, opts);
}

// ── Saved Searches ──────────────────────────────────────────────────────────

export function listSavedSearches(
  opts?: VaultRequestOptions,
): Promise<Schema["SavedSearchesListResponse"]> {
  return apiClient.get<Schema["SavedSearchesListResponse"]>("/v1/saved-searches", opts);
}

export function createSavedSearch(body: Schema["SavedSearchBody"]): Promise<Schema["SavedSearch"]> {
  return apiClient.post<Schema["SavedSearch"]>("/v1/saved-searches", body);
}

export function updateSavedSearch(
  id: number,
  body: Schema["SavedSearchBody"],
): Promise<Schema["SavedSearch"]> {
  return apiClient.patch<Schema["SavedSearch"]>(`/v1/saved-searches/${id}`, body);
}

export function deleteSavedSearch(id: number): Promise<void> {
  return apiClient.delete<void>(`/v1/saved-searches/${id}`);
}

// ── Search ──────────────────────────────────────────────────────────────────

/** The words the search language accepts on one list. */
export function listSearchFields(
  list: Schema["ListKind"],
  opts?: VaultRequestOptions,
): Promise<Schema["SearchFieldsResponse"]> {
  return apiClient.get<Schema["SearchFieldsResponse"]>(
    withQuery("/v1/search-fields", query({ list })),
    opts,
  );
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

export function getImport(
  id: number,
  opts?: VaultRequestOptions,
): Promise<Schema["ImportDetailResponse"]> {
  return apiClient.get<Schema["ImportDetailResponse"]>(`/v1/imports/${id}`, opts);
}

export function createImport(
  body: Schema["CreateImportBody"],
): Promise<Schema["CreateImportResponse"]> {
  return apiClient.post<Schema["CreateImportResponse"]>("/v1/imports", body);
}

export function setImportStage(
  id: number,
  body: Schema["SetImportStageBody"],
): Promise<Schema["SetImportStageResponse"]> {
  return apiClient.post<Schema["SetImportStageResponse"]>(`/v1/imports/${id}/stage`, body);
}

export function completeImport(
  id: number,
  body: Schema["CompleteImportBody"],
): Promise<Schema["CompleteImportResponse"]> {
  return apiClient.post<Schema["CompleteImportResponse"]>(`/v1/imports/${id}/complete`, body);
}

export function discardImport(id: number): Promise<Schema["DiscardImportResponse"]> {
  return apiClient.post<Schema["DiscardImportResponse"]>(`/v1/imports/${id}/discard`, {});
}

export function getImportContacts(
  id: number,
  opts?: VaultRequestOptions,
): Promise<Schema["ImportContactsResponse"]> {
  return apiClient.get<Schema["ImportContactsResponse"]>(`/v1/imports/${id}/contacts`, opts);
}

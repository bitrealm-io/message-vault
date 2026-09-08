/**
 * The only place in the suite that names vault URLs.
 *
 * Every other test fakes these functions by name, so nothing else notices when
 * a route is renamed. That makes this file the one thing standing between a
 * server-side rename and a screen that silently asks for the wrong address —
 * which is exactly the failure the old URL-matching tests could not catch,
 * because a renamed route made their comparisons stop matching rather than
 * fail.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";
import { apiClient } from "./api";
import {
  changePassword,
  createAccount,
  createApiToken,
  createContactGroup,
  createMessageTag,
  deleteAccount,
  deleteAccountById,
  deleteAccountMessages,
  deleteAllMessages,
  deleteApiToken,
  deleteContactGroup,
  deleteMessageTag,
  discardImport,
  getAccountProfile,
  getAccountStorage,
  getContact,
  getConversation,
  getConversationSources,
  getImport,
  listAccounts,
  listApiTokens,
  listContactGroupMembers,
  listContactGroups,
  listContacts,
  listConversationMessages,
  listConversations,
  listMessageTagMembers,
  listMessageTags,
  listSavedSearches,
  listSearchFields,
  setAccountPassword,
  setImportStage,
  updateAccount,
  updateAccountProfile,
  updateContact,
  updateContactGroup,
  updateContactGroupMembers,
  updateMessageTag,
  updateMessageTagMembers,
  updateSavedSearch,
} from "./vaultApi";

vi.mock("./api", () => ({
  apiClient: {
    get: vi.fn().mockResolvedValue({}),
    post: vi.fn().mockResolvedValue({}),
    put: vi.fn().mockResolvedValue({}),
    patch: vi.fn().mockResolvedValue({}),
    delete: vi.fn().mockResolvedValue({}),
  },
  // The signed-in account, as `auth.tsx` records it after sign-in.
  getAccountId: () => 7,
}));

const get = vi.mocked(apiClient.get);
const post = vi.mocked(apiClient.post);
const put = vi.mocked(apiClient.put);
const patch = vi.mocked(apiClient.patch);
const del = vi.mocked(apiClient.delete);

beforeEach(() => {
  vi.clearAllMocks();
});

/** The path the last call was made with, without its query string. */
function lastPath(mock: { mock: { calls: unknown[][] } }): string {
  return String(mock.mock.calls.at(-1)?.[0]).split("?")[0];
}

/** The query of the last call, as a plain object. */
function lastQuery(mock: { mock: { calls: unknown[][] } }): Record<string, string> {
  const url = String(mock.mock.calls.at(-1)?.[0]);
  const qs = url.includes("?") ? url.slice(url.indexOf("?") + 1) : "";
  return Object.fromEntries(new URLSearchParams(qs));
}

describe("browse routes", () => {
  it("reads conversations from /v1/conversations, not from an export path", async () => {
    await listConversations({ q: "trashed:yes", limit: 40, offset: 0 });
    expect(lastPath(get)).toBe("/v1/conversations");
  });

  it("reads contacts from /v1/contacts", async () => {
    await listContacts({ q: "" });
    expect(lastPath(get)).toBe("/v1/contacts");
  });

  it("addresses one contact by id", async () => {
    await getContact(42);
    expect(lastPath(get)).toBe("/v1/contacts/42");
  });

  it("addresses one conversation by id", async () => {
    await getConversation(12);
    expect(get).toHaveBeenCalledWith("/v1/conversations/12", undefined);
  });

  it("addresses a conversation's sources by id", async () => {
    await getConversationSources(12);
    expect(lastPath(get)).toBe("/v1/conversations/12/sources");
  });

  it("reads a conversation's messages from its own route, paged", async () => {
    await listConversationMessages(12, { offset: 50, limit: 50 });
    expect(lastPath(get)).toBe("/v1/conversations/12/messages");
    expect(lastQuery(get)).toEqual({ offset: "50", limit: "50" });
  });

  // The point of the read route: a year is a `year=` parameter, not a
  // `date:YYYY` term smuggled through the search language.
  it("narrows a conversation's messages with year=, not with date:YYYY", async () => {
    await listConversationMessages(12, { offset: 0, limit: 500, year: 2020 });
    expect(lastPath(get)).toBe("/v1/conversations/12/messages");
    expect(lastQuery(get)).toEqual({ offset: "0", limit: "500", year: "2020" });
    expect(String(get.mock.calls.at(-1)?.[0])).not.toContain("date:");
  });

  it("listSearchFields asks for one list's words", async () => {
    await listSearchFields("contacts");
    expect(get).toHaveBeenCalledWith("/v1/search-fields?list=contacts", undefined);
  });
});

describe("query building", () => {
  it("leaves absent, null, and empty values off the query string", async () => {
    await listConversations({ q: "", limit: 40, offset: 0, sort: undefined });
    expect(lastQuery(get)).toEqual({ limit: "40", offset: "0" });
  });

  it("keeps offset zero, which is a real page and not an absent value", async () => {
    await listContacts({ q: "ada", offset: 0, limit: 10 });
    expect(lastQuery(get)).toEqual({ q: "ada", offset: "0", limit: "10" });
  });

  it("emits no question mark when every value is absent", async () => {
    await listContacts({});
    expect(get.mock.calls.at(-1)?.[0]).toBe("/v1/contacts");
  });

  it("encodes a query that contains spaces and colons", async () => {
    await listConversations({ q: "in:#A attachment:any" });
    expect(lastQuery(get)).toEqual({ q: "in:#A attachment:any" });
  });
});

describe("verbs", () => {
  it("edits a contact with PATCH, since it changes one that already exists", async () => {
    await updateContact(7, { name: "Ada" });
    expect(patch).toHaveBeenCalledWith("/v1/contacts/7", { name: "Ada" });
    expect(post).not.toHaveBeenCalled();
  });

  it("updates a saved search with PATCH at its own id", async () => {
    await updateSavedSearch(3, { name: "Family", query: "kind:group" });
    expect(patch).toHaveBeenCalledWith("/v1/saved-searches/3", {
      name: "Family",
      query: "kind:group",
    });
  });

  it("deletes an API token at its own id, under the signed-in account", async () => {
    await deleteApiToken(1);
    expect(del).toHaveBeenCalledWith("/v1/accounts/7/api-tokens/1");
  });

  it("reads saved searches with GET", async () => {
    await listSavedSearches();
    expect(lastPath(get)).toBe("/v1/saved-searches");
  });
});

describe("import session routes", () => {
  it("patches the run to move its stage", async () => {
    await setImportStage(9, { stage: "parse" });
    expect(patch).toHaveBeenCalledWith("/v1/imports/9", { stage: "parse" });
  });

  it("addresses a discard by session id", async () => {
    await discardImport(9);
    expect(post).toHaveBeenCalledWith("/v1/imports/9/discard", {});
  });

  it("addresses one past run by id", async () => {
    await getImport(12);
    expect(lastPath(get)).toBe("/v1/imports/12");
  });
});

describe("Contact Groups and Message Tags are addressed by id", () => {
  it("lists and creates on the collection", async () => {
    await listContactGroups();
    expect(lastPath(get)).toBe("/v1/contact-groups");
    await createMessageTag({ name: "Holiday" });
    expect(post).toHaveBeenCalledWith("/v1/message-tags", { name: "Holiday" }, undefined);
  });

  it("renames with PATCH on the id and deletes with DELETE on the id", async () => {
    await updateContactGroup(12, { name: "Fam" });
    expect(patch).toHaveBeenCalledWith("/v1/contact-groups/12", { name: "Fam" }, undefined);
    await deleteMessageTag(7);
    expect(del).toHaveBeenCalledWith("/v1/message-tags/7", undefined, undefined);
  });

  it("reads and patches membership under the set", async () => {
    await listMessageTagMembers(7);
    expect(lastPath(get)).toBe("/v1/message-tags/7/members");
    await updateContactGroupMembers(12, { add: [1, 2], remove: [3] });
    expect(patch).toHaveBeenCalledWith(
      "/v1/contact-groups/12/members",
      { add: [1, 2], remove: [3] },
      undefined,
    );
  });

  it("passes the abort options through on a write", async () => {
    const controller = new AbortController();
    await deleteContactGroup(12, { signal: controller.signal });
    expect(del).toHaveBeenCalledWith("/v1/contact-groups/12", undefined, {
      signal: controller.signal,
    });
  });

  it("covers the other five of the twelve, so every URL is named here", async () => {
    await createContactGroup({ name: "Family" });
    expect(post).toHaveBeenCalledWith("/v1/contact-groups", { name: "Family" }, undefined);
    await listContactGroupMembers(12);
    expect(lastPath(get)).toBe("/v1/contact-groups/12/members");
    await listMessageTags();
    expect(lastPath(get)).toBe("/v1/message-tags");
    await updateMessageTag(7, { name: "Hot" });
    expect(patch).toHaveBeenCalledWith("/v1/message-tags/7", { name: "Hot" }, undefined);
    await updateMessageTagMembers(7, { add: [1], remove: [] });
    expect(patch).toHaveBeenCalledWith(
      "/v1/message-tags/7/members",
      { add: [1], remove: [] },
      undefined,
    );
  });
});

describe("accounts are one collection", () => {
  it("lists and creates on /v1/accounts, for the owner and for a stranger alike", async () => {
    await listAccounts();
    expect(lastPath(get)).toBe("/v1/accounts");
    await createAccount({ username: "carol", password: "hunter2hunter2" });
    expect(post).toHaveBeenCalledWith("/v1/accounts", {
      username: "carol",
      password: "hunter2hunter2",
    });
  });

  it("addresses the signed-in account by the id the session carries", async () => {
    await getAccountProfile();
    expect(lastPath(get)).toBe("/v1/accounts/7");
    await updateAccountProfile({ preferred_name: "Ada" });
    expect(patch).toHaveBeenCalledWith("/v1/accounts/7", { preferred_name: "Ada" });
    await changePassword({ current_password: "old", password: "newer-one" });
    expect(put).toHaveBeenCalledWith("/v1/accounts/7/password", {
      current_password: "old",
      password: "newer-one",
    });
    await deleteAccount({ confirm: true, current_password: "old" });
    expect(del).toHaveBeenCalledWith("/v1/accounts/7", { confirm: true, current_password: "old" });
    await deleteAllMessages({ confirm: true });
    expect(del).toHaveBeenCalledWith("/v1/accounts/7/messages", { confirm: true });
    await getAccountStorage();
    expect(lastPath(get)).toBe("/v1/accounts/7/storage");
  });

  it("addresses another account by the id the owner names", async () => {
    await updateAccount(12, { disabled: true });
    expect(patch).toHaveBeenCalledWith("/v1/accounts/12", { disabled: true });
    await setAccountPassword(12, { password: "resetbytheowner" });
    expect(put).toHaveBeenCalledWith("/v1/accounts/12/password", { password: "resetbytheowner" });
    await deleteAccountMessages(12);
    expect(del).toHaveBeenCalledWith("/v1/accounts/12/messages");
    await deleteAccountById(12);
    expect(del).toHaveBeenCalledWith("/v1/accounts/12");
  });

  it("keeps API tokens under the signed-in account", async () => {
    await listApiTokens();
    expect(lastPath(get)).toBe("/v1/accounts/7/api-tokens");
    await createApiToken({ label: "cli" });
    expect(post).toHaveBeenCalledWith("/v1/accounts/7/api-tokens", { label: "cli" });
  });
});

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
  createContactGroup,
  createMessageTag,
  deleteApiToken,
  deleteContactGroup,
  deleteMessageTag,
  discardImport,
  getContact,
  getConversation,
  getConversationSources,
  getImport,
  listContactGroupMembers,
  listContactGroups,
  listContacts,
  listConversationMessages,
  listConversations,
  listMessageTagMembers,
  listMessageTags,
  listSavedSearches,
  listSearchFields,
  setImportStage,
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
}));

const get = vi.mocked(apiClient.get);
const post = vi.mocked(apiClient.post);
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

  it("deletes an API token at its own id", async () => {
    await deleteApiToken("tok_1");
    expect(del).toHaveBeenCalledWith("/v1/account/api-tokens/tok_1");
  });

  it("reads saved searches with GET", async () => {
    await listSavedSearches();
    expect(lastPath(get)).toBe("/v1/saved-searches");
  });
});

describe("import session routes", () => {
  it("addresses a stage change by session id", async () => {
    await setImportStage(9, { stage: "parse" });
    expect(post).toHaveBeenCalledWith("/v1/imports/9/stage", { stage: "parse" });
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

describe("path parameters are escaped", () => {
  it("escapes an id containing a slash rather than building a deeper path", async () => {
    await deleteApiToken("a/b");
    expect(del).toHaveBeenCalledWith("/v1/account/api-tokens/a%2Fb");
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

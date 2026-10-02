/**
 * The shape of a cache key, which is the only thing this module has.
 *
 * What matters is not the words in a key but which keys sit under which
 * prefix: TanStack Query finds entries by prefix, so `keys.contacts.lists`
 * being a prefix of every contact list is what lets one optimistic write patch
 * the chips on all of them.
 */

import { describe, expect, it } from "vitest";
import { keys } from "./queryKeys";

/** True when `prefix` finds `key`. */
function coveredBy(key: readonly unknown[], prefix: readonly unknown[]): boolean {
  return prefix.every((part, i) => key[i] === part);
}

describe("keys", () => {
  it("puts a contact list page and one contact under the contacts prefix", () => {
    expect(keys.contacts.list("ada")).toEqual(["contacts", "list", "ada"]);
    expect(keys.contacts.detail(12)).toEqual(["contacts", "detail", "12"]);
    expect(coveredBy(keys.contacts.list("ada"), keys.contacts.all)).toBe(true);
    expect(coveredBy(keys.contacts.detail(12), keys.contacts.all)).toBe(true);
    // The list prefix leaves an open drawer out, so the chip patch for a row
    // of a paged list and for one contact stay two targets.
    expect(coveredBy(keys.contacts.detail(12), keys.contacts.lists)).toBe(false);
  });

  it("keeps the trashed contact list under the list prefix without sharing an entry", () => {
    const trashed = keys.contacts.trashed("trashed:yes");
    // Everything that lists contacts sits under one prefix.
    expect(coveredBy(trashed, keys.contacts.lists)).toBe(true);
    // The Trash screen holds one page and the contact list holds paged
    // `InfiniteData`, so no search term may make the two keys equal.
    expect(trashed).not.toEqual(keys.contacts.list("trashed:yes"));
    expect(trashed).not.toEqual(keys.contacts.list("trashed"));
  });

  it('carries a contact id as text, so 12 and "12" name one entry', () => {
    expect(keys.contacts.detail(12)).toEqual(keys.contacts.detail("12"));
  });

  it("gives each search and sort of the conversation list its own entry", () => {
    const key = keys.conversations.list({ q: "tag:Holiday", sort: "date", order: "desc" });
    expect(key).toEqual(["conversations", "list", "tag:Holiday", "date", "desc"]);
    expect(key).not.toEqual(
      keys.conversations.list({ q: "tag:Holiday", sort: "date", order: "asc" }),
    );
    expect(coveredBy(key, keys.conversations.all)).toBe(true);
    expect(coveredBy(keys.conversations.sources(7), keys.conversations.all)).toBe(true);
  });

  it("gives every named collection a prefix of its own", () => {
    expect(keys.contactGroups.all).toEqual(["contact-groups"]);
    expect(keys.messageTags.all).toEqual(["message-tags"]);
    expect(keys.savedSearches.all).toEqual(["saved-searches"]);
    expect(keys.accountProfile.all).toEqual(["account-profile"]);
    expect(keys.apiTokens.all).toEqual(["api-tokens"]);
    expect(keys.ownerAccounts.all).toEqual(["owner-accounts"]);
    expect(keys.serverSettings.all).toEqual(["server-settings"]);
  });

  it("puts each list's search words under one prefix", () => {
    expect(keys.searchFields.list("contacts")).toEqual(["search-fields", "contacts"]);
    expect(coveredBy(keys.searchFields.list("contacts"), keys.searchFields.all)).toBe(true);
  });

  it("puts the storage overview and one import run under the storage prefix", () => {
    expect(keys.storage.overview).toEqual(["storage", "overview"]);
    expect(keys.storage.importDetail(4)).toEqual(["storage", "import", "4"]);
    expect(coveredBy(keys.storage.overview, keys.storage.all)).toBe(true);
    expect(coveredBy(keys.storage.importDetail(4), keys.storage.all)).toBe(true);
  });

  it("puts every trash count under one prefix", () => {
    expect(keys.trash.count("trashed:yes")).toEqual(["trash", "count", "trashed:yes"]);
    expect(coveredBy(keys.trash.count("trashed:yes"), keys.trash.all)).toBe(true);
  });
});

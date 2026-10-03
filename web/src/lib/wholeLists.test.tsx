/** @vitest-environment jsdom */

/**
 * A list the web app reads whole is read page by page until it has `total` rows.
 *
 * Every `/v1` list answers one page, 40 rows when no `limit` is sent, 500 at
 * most. These tests run the real route functions over a fake transport that
 * pages the way the server does, so a list read as one request stops at the
 * fortieth row here exactly as it does in the app (issue #1145).
 */

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { mockedAuth, Providers } from "../test/providers";
import { apiClient } from "./api";
import { useContactGroupActions } from "./contactGroups";
import {
  getConversationSources,
  listAccountIdentities,
  listAccounts,
  listApiTokens,
  listContactGroups,
  listMessageTags,
  listSavedSearches,
  listSearchFields,
} from "./serverApi";
import { useContactGroups } from "./useContactGroups";

vi.mock("./auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("./api", () => ({
  apiClient: {
    get: vi.fn(),
    post: vi.fn(),
    put: vi.fn(),
    patch: vi.fn(),
    delete: vi.fn(),
  },
  getAccountId: () => 7,
}));

const get = vi.mocked(apiClient.get);
const patch = vi.mocked(apiClient.patch);

/** The server's paging: 40 rows without a `limit`, never more than 500. */
function servePages(rows: readonly unknown[]) {
  get.mockImplementation(async (url: string) => {
    const qs = new URLSearchParams(url.includes("?") ? url.slice(url.indexOf("?") + 1) : "");
    const limit = Number(qs.get("limit") ?? 40);
    const offset = Number(qs.get("offset") ?? 0);
    if (limit < 1 || limit > 500) throw new Error(`limit ${limit} is out of range`);
    return { items: rows.slice(offset, offset + limit), total: rows.length, limit, offset };
  });
}

const groups = Array.from({ length: 41 }, (_, i) => ({
  id: i + 1,
  name: `Group ${String(i + 1).padStart(2, "0")}`,
}));

beforeEach(() => {
  vi.clearAllMocks();
});

describe("the lists read whole", () => {
  it("lists the forty-first Contact Group", async () => {
    servePages(groups);
    const { result } = renderHook(() => useContactGroups(), { wrapper: Providers });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.groups).toHaveLength(41);
    expect(result.current.groups).toContain("Group 41");
  });

  it("finds the id of the forty-first Contact Group by its name", async () => {
    servePages(groups);
    patch.mockResolvedValue({ id: 41, name: "Renamed" });
    const { result } = renderHook(() => useContactGroupActions(), { wrapper: Providers });
    await act(async () => {
      await result.current.rename("Group 41", "Renamed");
    });
    expect(patch).toHaveBeenCalledWith("/v1/contact-groups/41", { name: "Renamed" }, undefined);
  });

  it("reads every row of each list a screen takes as the whole list", async () => {
    const rows = Array.from({ length: 1201 }, (_, i) => ({ id: i + 1, name: `Row ${i + 1}` }));
    servePages(rows);
    for (const read of [
      () => listContactGroups(),
      () => listMessageTags(),
      () => listSavedSearches(),
      () => listApiTokens(),
      () => listAccounts(),
      () => listAccountIdentities(),
      () => listSearchFields("contacts"),
      () => getConversationSources(12),
    ]) {
      expect(await read()).toHaveLength(1201);
    }
  });

  it("asks for pages of the server's maximum", async () => {
    servePages(groups);
    await listContactGroups();
    expect(String(get.mock.calls[0]?.[0])).toBe("/v1/contact-groups?limit=500&offset=0");
  });

  it("stops at an empty page even when the total says more rows exist", async () => {
    // A list that shrank between two pages must not loop forever.
    get.mockResolvedValueOnce({ items: [{ id: 1 }], total: 3, limit: 500, offset: 0 });
    get.mockResolvedValueOnce({ items: [], total: 3, limit: 500, offset: 1 });
    expect(await listContactGroups()).toEqual([{ id: 1 }]);
    expect(get).toHaveBeenCalledTimes(2);
  });
});

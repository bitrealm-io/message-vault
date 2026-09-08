/**
 * Every route function, checked against the OpenAPI document the vault
 * publishes.
 *
 * `vaultApi.ts` says at the top that its types come from
 * `docs/src/assets/openapi.json`, and a vault-side test pins that document to
 * the running server. Nothing checked the *paths* against it. `vaultApi.test.ts`
 * pins 25 of the 67 functions by hand, so the other 42 could ask for an address
 * the vault does not serve and no test in the repository would notice — the
 * screens all fake these functions by name.
 *
 * This is the check, and it needs no expected path of its own: the document is
 * the expectation. Each function is called with plausible arguments through a
 * faked `apiClient`, and the method and path it asked for must appear in
 * `openapi.json`. Renaming a route on the server side, regenerating the
 * document, and forgetting to update this module fails here.
 *
 * `EXERCISED` must name every exported function, which the last test enforces,
 * so a new route cannot be added without being covered.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { apiClient } from "./api";
import * as vaultApi from "./vaultApi";

vi.mock("./api", () => ({
  apiClient: {
    get: vi.fn().mockResolvedValue({}),
    post: vi.fn().mockResolvedValue({}),
    postRaw: vi.fn().mockResolvedValue({}),
    put: vi.fn().mockResolvedValue({}),
    patch: vi.fn().mockResolvedValue({}),
    delete: vi.fn().mockResolvedValue({}),
  },
  getAccountId: () => 7,
  getBaseUrl: () => "",
  getToken: () => "mv-user-test",
  problemFromBody: (status: number, text: string) => new Error(`${status}: ${text}`),
  VaultApiError: Error,
}));

type OpenApiDocument = { paths: Record<string, Record<string, unknown>> };

const OPENAPI_PATH = fileURLToPath(
  new URL("../../../docs/src/assets/openapi.json", import.meta.url),
);
const openapi = JSON.parse(readFileSync(OPENAPI_PATH, "utf8")) as OpenApiDocument;

/**
 * One matcher per documented path: `{id}` and friends stand for a single path
 * segment, so `/v1/contacts/{id}` accepts `/v1/contacts/42` but not
 * `/v1/contacts/42/trash`, which is a documented path of its own.
 */
const DOCUMENTED = Object.entries(openapi.paths).map(([template, item]) => ({
  template,
  methods: new Set(Object.keys(item).map((m) => m.toUpperCase())),
  matches: new RegExp(`^${template.replace(/\{[^}]+\}/g, "[^/]+")}$`),
}));

/** `postRaw` is a POST that carries its own media type. */
const VERB_METHOD: Record<string, string> = {
  get: "GET",
  post: "POST",
  postRaw: "POST",
  put: "PUT",
  patch: "PATCH",
  delete: "DELETE",
};

/** The method and path of the single call the function under test made. */
function calledRoute(): { method: string; path: string } {
  const calls = Object.entries(VERB_METHOD).flatMap(([verb, method]) =>
    vi
      .mocked(apiClient[verb as keyof typeof apiClient])
      .mock.calls.map((args) => ({ method, path: String(args[0]) })),
  );
  expect(calls).toHaveLength(1);
  const { method, path } = calls[0];
  return { method, path: path.split("?")[0] };
}

/** Plausible arguments for every route function, one call each. */
const EXERCISED: Record<string, () => unknown> = {
  // Session and vault
  login: () => vaultApi.login({ username: "matt", password: "hunter2hunter2" }),
  getSession: () => vaultApi.getSession(),
  logout: () => vaultApi.logout(),
  getVaultState: () => vaultApi.getVaultState(),
  claimVault: () => vaultApi.claimVault({ username: "matt", password: "hunter2hunter2" }),
  getVaultSettings: () => vaultApi.getVaultSettings(),
  updateVaultSettings: () => vaultApi.updateVaultSettings({ public_registration: true }),

  // Accounts
  listAccounts: () => vaultApi.listAccounts(),
  createAccount: () => vaultApi.createAccount({ username: "matt", password: "hunter2hunter2" }),
  updateAccount: () => vaultApi.updateAccount(9, { preferred_name: "Matt" }),
  setAccountPassword: () => vaultApi.setAccountPassword(9, { password: "hunter2hunter2" }),
  deleteAccountById: () => vaultApi.deleteAccountById(9),
  deleteAccountMessages: () => vaultApi.deleteAccountMessages(9),
  getAccountProfile: () => vaultApi.getAccountProfile(),
  updateAccountProfile: () => vaultApi.updateAccountProfile({ preferred_name: "Matt" }),
  changePassword: () =>
    vaultApi.changePassword({
      current_password: "hunter2hunter2",
      password: "hunter3hunter3",
    }),
  deleteAccount: () =>
    vaultApi.deleteAccount({ confirm: true, current_password: "hunter2hunter2" }),
  getAccountStorage: () => vaultApi.getAccountStorage(),
  deleteAllMessages: () => vaultApi.deleteAllMessages({ confirm: true }),

  // API tokens
  listApiTokens: () => vaultApi.listApiTokens(),
  createApiToken: () =>
    vaultApi.createApiToken({
      label: "backup client",
      can_import: true,
      can_export: true,
      can_delete: false,
    }),
  renameApiToken: () => vaultApi.renameApiToken(3, { label: "renamed" }),
  deleteApiToken: () => vaultApi.deleteApiToken(3),

  // Browse
  listConversations: () => vaultApi.listConversations({ q: "", limit: 40, offset: 0 }),
  getConversation: () => vaultApi.getConversation(12),
  listConversationMessages: () => vaultApi.listConversationMessages(12, { offset: 0, limit: 50 }),
  listMessages: () => vaultApi.listMessages({ q: "", limit: 40, offset: 0 }),
  getConversationSources: () => vaultApi.getConversationSources(12),
  trashConversation: () => vaultApi.trashConversation(12),
  restoreConversation: () => vaultApi.restoreConversation(12),
  deleteConversation: () => vaultApi.deleteConversation(12),
  emptyTrash: () => vaultApi.emptyTrash(),

  // Contacts
  listContacts: () => vaultApi.listContacts({ q: "" }),
  getContact: () => vaultApi.getContact(42),
  updateContact: () => vaultApi.updateContact(42, { name: "Sam" }),
  getContactSummaries: () => vaultApi.getContactSummaries({ ids: [1, 2] }),
  unmatchedHandles: () => vaultApi.unmatchedHandles({ identifiers: ["+15555550100"] }),
  loadAddressBook: () => vaultApi.loadAddressBook("BEGIN:VCARD\nEND:VCARD\n", "text/vcard"),
  trashContact: () => vaultApi.trashContact(42),
  restoreContact: () => vaultApi.restoreContact(42),
  deleteContact: () => vaultApi.deleteContact(42),

  // Contact groups
  listContactGroups: () => vaultApi.listContactGroups(),
  createContactGroup: () => vaultApi.createContactGroup({ name: "Family" }),
  updateContactGroup: () => vaultApi.updateContactGroup(5, { name: "Family" }),
  deleteContactGroup: () => vaultApi.deleteContactGroup(5),
  listContactGroupMembers: () => vaultApi.listContactGroupMembers(5),
  updateContactGroupMembers: () => vaultApi.updateContactGroupMembers(5, { add: [1], remove: [] }),

  // Message tags
  listMessageTags: () => vaultApi.listMessageTags(),
  createMessageTag: () => vaultApi.createMessageTag({ name: "Receipts" }),
  updateMessageTag: () => vaultApi.updateMessageTag(6, { name: "Receipts" }),
  deleteMessageTag: () => vaultApi.deleteMessageTag(6),
  listMessageTagMembers: () => vaultApi.listMessageTagMembers(6),
  updateMessageTagMembers: () => vaultApi.updateMessageTagMembers(6, { add: [1], remove: [] }),

  // Saved searches
  listSavedSearches: () => vaultApi.listSavedSearches(),
  createSavedSearch: () => vaultApi.createSavedSearch({ name: "Receipts", query: "receipt" }),
  updateSavedSearch: () => vaultApi.updateSavedSearch(8, { name: "Receipts", query: "receipt" }),
  deleteSavedSearch: () => vaultApi.deleteSavedSearch(8),
  listSearchFields: () => vaultApi.listSearchFields("contacts"),

  // Imports
  listImports: () => vaultApi.listImports(),
  getImport: () => vaultApi.getImport(4),
  createImport: () => vaultApi.createImport({ source: "iPhone" }),
  setImportStage: () => vaultApi.setImportStage(4, { stage: "staged" }),
  completeImport: () => vaultApi.completeImport(4, {}),
  discardImport: () => vaultApi.discardImport(4),
  getImportContacts: () => vaultApi.getImportContacts(4),
};

/**
 * Not a route function: it builds an asset URL and fetches it directly, so it
 * never reaches `apiClient` and has no single documented path to check. Its own
 * behaviour is covered in `vaultApi.test.ts`.
 */
const NOT_ROUTED = new Set(["fetchAssetObjectUrl", "addressBookContentType"]);

beforeEach(() => {
  vi.clearAllMocks();
});

describe("every route function asks for a documented address", () => {
  for (const [name, call] of Object.entries(EXERCISED)) {
    it(`${name} matches a path in openapi.json`, async () => {
      await call();
      const { method, path } = calledRoute();
      const documented = DOCUMENTED.find((p) => p.matches.test(path));
      expect(
        documented,
        `${name} asked for ${method} ${path}, which openapi.json does not document`,
      ).toBeDefined();
      expect(
        documented?.methods.has(method),
        `${name} asked for ${method} ${path}; openapi.json documents ` +
          `${[...(documented?.methods ?? [])].join(", ")} on ${documented?.template}`,
      ).toBe(true);
    });
  }
});

describe("the table stays complete", () => {
  it("names every exported route function", () => {
    const exported = Object.entries(vaultApi)
      .filter(([, value]) => typeof value === "function")
      .map(([name]) => name)
      .filter((name) => !NOT_ROUTED.has(name))
      .sort();
    const covered = Object.keys(EXERCISED).sort();
    expect(exported).toEqual(covered);
  });

  it("reads a document that actually has paths, so a missing file cannot pass it", () => {
    expect(DOCUMENTED.length).toBeGreaterThan(40);
    expect(DOCUMENTED.some((p) => p.template === "/v1/session")).toBe(true);
  });
});

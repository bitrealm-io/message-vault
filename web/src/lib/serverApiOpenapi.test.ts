/**
 * Every route function, checked against the OpenAPI document the server
 * publishes.
 *
 * `serverApi.ts` says at the top that its types come from
 * `docs/src/assets/openapi.json`, and a server-side test pins that document to
 * the running server. Nothing checked the *paths* against it. `serverApi.test.ts`
 * pins 42 of the 79 functions by hand, so the other 37 could ask for an address
 * the server does not serve and no test in the repository would notice — the
 * screens all fake these functions by name.
 *
 * This is the check, and it needs no expected path of its own: the document is
 * the expectation. Each function is called with plausible arguments through a
 * faked `apiClient`, and the method and path it asked for must appear in
 * `openapi.json`, and every query parameter it sent must be one that
 * operation declares, because the server refuses any other with a 422.
 * Renaming a route or a parameter on the server side, regenerating the
 * document, and forgetting to update this module fails here.
 *
 * `EXERCISED` must name every exported function, which the last test enforces,
 * so a new route cannot be added without being covered.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { apiClient } from "./api";
import * as serverApi from "./serverApi";

vi.mock("./api", () => ({
  apiClient: {
    get: vi.fn().mockResolvedValue({}),
    post: vi.fn().mockResolvedValue({}),
    postRaw: vi.fn().mockResolvedValue({}),
    postText: vi.fn().mockResolvedValue(""),
    put: vi.fn().mockResolvedValue({}),
    patch: vi.fn().mockResolvedValue({}),
    delete: vi.fn().mockResolvedValue({}),
  },
  getAccountId: () => 7,
  getBaseUrl: () => "",
  getToken: () => "mc-user-test",
  problemFromBody: (status: number, text: string) => new Error(`${status}: ${text}`),
  ApiError: Error,
}));

type OpenApiOperation = { parameters?: { name: string; in: string }[] };
type OpenApiDocument = { paths: Record<string, Record<string, OpenApiOperation>> };

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
  /** The query parameters each method's operation declares. */
  queryParams: (method: string) =>
    new Set(
      (item[method.toLowerCase()]?.parameters ?? [])
        .filter((p) => p.in === "query")
        .map((p) => p.name),
    ),
}));

/** `postRaw` is a POST that carries its own media type; `postText` is one that reads a file back. */
const VERB_METHOD: Record<string, string> = {
  get: "GET",
  post: "POST",
  postRaw: "POST",
  postText: "POST",
  put: "PUT",
  patch: "PATCH",
  delete: "DELETE",
};

/** The method, path and query parameter names of the single call the function under test made. */
function calledRoute(): { method: string; path: string; query: string[] } {
  const calls = Object.entries(VERB_METHOD).flatMap(([verb, method]) =>
    vi
      .mocked(apiClient[verb as keyof typeof apiClient])
      .mock.calls.map((args) => ({ method, path: String(args[0]) })),
  );
  expect(calls).toHaveLength(1);
  const { method, path } = calls[0];
  const [bare, search = ""] = path.split("?");
  return { method, path: bare, query: [...new URLSearchParams(search).keys()] };
}

/** Plausible arguments for every route function, one call each. */
const EXERCISED: Record<string, () => unknown> = {
  // Session and server
  login: () => serverApi.login({ username: "matt", password: "hunter2hunter2" }),
  getSession: () => serverApi.getSession(),
  logout: () => serverApi.logout(),
  getServerState: () => serverApi.getServerState(),
  claimServer: () => serverApi.claimServer({ username: "matt", password: "hunter2hunter2" }),
  getServerSettings: () => serverApi.getServerSettings(),
  updateServerSettings: () => serverApi.updateServerSettings({ public_registration: true }),
  getServerStorage: () => serverApi.getServerStorage(),
  getDemoAccount: () => serverApi.getDemoAccount(),
  replaceDemoAccount: () => serverApi.replaceDemoAccount({ size: "medium" }),

  // Accounts
  listAccounts: () => serverApi.listAccounts(),
  createAccount: () => serverApi.createAccount({ username: "matt", password: "hunter2hunter2" }),
  updateAccount: () => serverApi.updateAccount(9, { preferred_name: "Matt" }),
  getAccount: () => serverApi.getAccount(9),
  setAccountPassword: () =>
    serverApi.setAccountPassword(9, {
      password: "hunter2hunter2",
      password_confirmation: "hunter2hunter2",
    }),
  deleteAccountById: () => serverApi.deleteAccountById(9),
  deleteAccountMessages: () => serverApi.deleteAccountMessages(9),
  getAccountProfile: () => serverApi.getAccountProfile(),
  updateAccountProfile: () => serverApi.updateAccountProfile({ preferred_name: "Matt" }),
  changePassword: () =>
    serverApi.changePassword({
      password: "hunter3hunter3",
      password_confirmation: "hunter3hunter3",
    }),
  deleteAccount: () =>
    serverApi.deleteAccount({ confirm: true, current_password: "hunter2hunter2" }),
  getAccountStorage: () => serverApi.getAccountStorage(),
  listAccountIdentities: () => serverApi.listAccountIdentities(undefined, 3),
  listAccountImports: () => serverApi.listAccountImports({ limit: 50, offset: 50 }, undefined, 3),
  getAccountImport: () => serverApi.getAccountImport(2, undefined, 3),
  listAccountExports: () => serverApi.listAccountExports({ limit: 50, offset: 50 }),
  deleteAllMessages: () => serverApi.deleteAllMessages({ confirm: true }),

  // API tokens
  listApiTokens: () => serverApi.listApiTokens(),
  createApiToken: () =>
    serverApi.createApiToken({
      label: "backup client",
      can_import: true,
      can_export: true,
    }),
  renameApiToken: () => serverApi.renameApiToken(3, { label: "renamed" }),
  deleteApiToken: () => serverApi.deleteApiToken(3),

  // Browse
  listConversations: () => serverApi.listConversations({ q: "", limit: 40, offset: 0 }),
  getConversation: () => serverApi.getConversation(12),
  listConversationMessages: () => serverApi.listConversationMessages(12, { offset: 0, limit: 50 }),
  listMessages: () => serverApi.listMessages({ q: "", limit: 40, offset: 0 }),
  getConversationSources: () => serverApi.getConversationSources(12),
  trashConversation: () => serverApi.trashConversation(12),
  restoreConversation: () => serverApi.restoreConversation(12),
  deleteConversation: () => serverApi.deleteConversation(12),
  emptyTrash: () => serverApi.emptyTrash(),

  // Contacts
  listContacts: () => serverApi.listContacts({ q: "" }),
  getContact: () => serverApi.getContact(42),
  updateContact: () => serverApi.updateContact(42, { name: "Sam" }),
  getContactSummaries: () => serverApi.getContactSummaries({ ids: [1, 2] }),
  unmatchedIdentities: () => serverApi.unmatchedIdentities({ identifiers: ["+15555550100"] }),
  loadAddressBook: () => serverApi.loadAddressBook("contact_id,display_name\n", "edit"),
  exportAddressBook: () => serverApi.exportAddressBook({ q: "group:Family", ids: [1, 2] }),
  trashContact: () => serverApi.trashContact(42),
  restoreContact: () => serverApi.restoreContact(42),
  deleteContact: () => serverApi.deleteContact(42),

  // Contact groups
  listContactGroups: () => serverApi.listContactGroups(),
  createContactGroup: () => serverApi.createContactGroup({ name: "Family" }),
  updateContactGroup: () => serverApi.updateContactGroup(5, { name: "Family" }),
  deleteContactGroup: () => serverApi.deleteContactGroup(5),
  listContactGroupMembers: () => serverApi.listContactGroupMembers(5),
  updateContactGroupMembers: () => serverApi.updateContactGroupMembers(5, { add: [1], remove: [] }),

  // Message tags
  listMessageTags: () => serverApi.listMessageTags(),
  createMessageTag: () => serverApi.createMessageTag({ name: "Receipts" }),
  updateMessageTag: () => serverApi.updateMessageTag(6, { name: "Receipts" }),
  deleteMessageTag: () => serverApi.deleteMessageTag(6),
  listMessageTagMembers: () => serverApi.listMessageTagMembers(6),
  updateMessageTagMembers: () => serverApi.updateMessageTagMembers(6, { add: [1], remove: [] }),

  // Saved searches
  listSavedSearches: () => serverApi.listSavedSearches(),
  createSavedSearch: () => serverApi.createSavedSearch({ name: "Receipts", query: "receipt" }),
  updateSavedSearch: () => serverApi.updateSavedSearch(8, { name: "Receipts", query: "receipt" }),
  deleteSavedSearch: () => serverApi.deleteSavedSearch(8),
  listSearchFields: () => serverApi.listSearchFields("contacts"),

  // Exports
  getExport: () => serverApi.getExport(2),
  createExport: () =>
    serverApi.createExport({ scope: { kind: "everything" }, tool: "message-crate-pull" }),
  completeExport: () => serverApi.completeExport(2),
  cancelExport: () => serverApi.cancelExport(2),

  // Imports
  listImports: () => serverApi.listImports(),
  getImport: () => serverApi.getImport(4),
  createImport: () => serverApi.createImport({ source: "iPhone" }),
  setImportStage: () => serverApi.setImportStage(4, { stage: "staged" }),
  completeImport: () => serverApi.completeImport(4, { status: "completed" }),
  discardImport: () => serverApi.discardImport(4),
  getImportContacts: () => serverApi.getImportContacts(4),
};

/**
 * Not a route function: it builds an asset URL and fetches it directly, so it
 * never reaches `apiClient` and has no single documented path to check. No test
 * covers its own behaviour: the screen tests that use it fake it by name.
 */
const NOT_ROUTED = new Set(["fetchAssetObjectUrl"]);

beforeEach(() => {
  vi.clearAllMocks();
});

describe("every route function asks for a documented address", () => {
  for (const [name, call] of Object.entries(EXERCISED)) {
    it(`${name} matches a path in openapi.json`, async () => {
      await call();
      const { method, path, query } = calledRoute();
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
      const declared = documented?.queryParams(method) ?? new Set<string>();
      expect(
        query.filter((name) => !declared.has(name)),
        `${name} sent query parameters ${documented?.template} does not declare`,
      ).toEqual([]);
    });
  }
});

describe("the table stays complete", () => {
  it("names every exported route function", () => {
    const exported = Object.entries(serverApi)
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

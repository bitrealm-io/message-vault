import type { components } from "./vaultApi.types";

/**
 * The signed-in account as the vault returns it from `GET /v1/accounts/{id}`:
 * its profile, its flags, and how much it holds.
 *
 * Generated from the vault's own OpenAPI document rather than written here, so
 * a field renamed on the server becomes a build error instead of a screen that
 * silently shows nothing.
 */
export type AccountProfile = components["schemas"]["AccountResponse"];

import type { components } from "../../../lib/vaultApi.types";

/** What `GET /v1/vault/storage` answers: the figures every Dashboard section reads. */
export type VaultStorage = components["schemas"]["VaultStorageResponse"];

/** One account's row in Messages by account. */
export type AccountStorage = components["schemas"]["AccountMessagesResponse"];

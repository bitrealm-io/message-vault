/**
 * The permanent Contact Group holding what the vault could not identify: a
 * contact with no identity, or with identities and no preferred name. The
 * server computes its membership, so it is never stored on a contact. This is
 * the word the search language knows it by (`group:unknown`).
 */
export const UNKNOWN_GROUP = "unknown";

/** What the Unknown group is called in the sidebar and on a contact. */
export const UNKNOWN_GROUP_LABEL = "Unknown";

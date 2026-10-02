import { getAccountId } from "./api";
import { accountKey, readPref, removePref, writePref } from "./storage";

/**
 * Recent search queries, kept per account and per search bar. Per account, so
 * the next account on the same browser is never shown what the last one
 * searched for. Per bar, so the contacts, messages, and trash bars do not offer
 * each other's history — a `handle:` query is noise in the contacts bar, and a
 * contact name is noise in the messages bar. With no account logged in there
 * is no history: nothing is read and nothing is saved.
 */
export type SearchScope = "account" | "contact" | "message" | "trash";

/** The logged-in account's key for one bar's history, or null when logged out. */
function storageKey(scope: SearchScope): string | null {
  const accountId = getAccountId();
  if (accountId === null) return null;
  return accountKey(accountId, `${scope}-recent-searches`);
}

const RECENT_SEARCHES_MAX = 10;

function readRaw(scope: SearchScope): unknown {
  const key = storageKey(scope);
  if (key === null) return null;
  const raw = readPref(key);
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

/** Recent queries for one search bar, newest first. */
export function loadRecentSearches(scope: SearchScope): string[] {
  const parsed = readRaw(scope);
  if (!Array.isArray(parsed)) return [];
  return parsed
    .filter((x): x is string => typeof x === "string")
    .map((s) => s.trim())
    .filter(Boolean)
    .slice(0, RECENT_SEARCHES_MAX);
}

function saveRecentSearches(scope: SearchScope, queries: string[]): void {
  const key = storageKey(scope);
  if (key === null) return;
  writePref(key, JSON.stringify(queries.slice(0, RECENT_SEARCHES_MAX)));
}

/** Remove every saved query for one search bar. */
export function clearRecentSearches(scope: SearchScope): void {
  const key = storageKey(scope);
  if (key !== null) removePref(key);
}

/** Put this query at the front of a bar's recents, dropping duplicates. */
export function pushRecentSearch(scope: SearchScope, query: string): string[] {
  const q = query.trim();
  if (!q) return loadRecentSearches(scope);
  const next = [q, ...loadRecentSearches(scope).filter((x) => x !== q)].slice(
    0,
    RECENT_SEARCHES_MAX,
  );
  saveRecentSearches(scope, next);
  return next;
}

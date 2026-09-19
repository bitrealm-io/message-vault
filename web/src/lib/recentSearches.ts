/**
 * Recent search queries, kept per search bar so the contacts, messages, and
 * trash bars do not offer each other's history — a `handle:` query is noise in
 * the contacts bar, and a contact name is noise in the messages bar.
 */
export type SearchScope = "account" | "contact" | "message" | "trash";

/**
 * `contact` keeps the key it shipped with so existing history survives the move
 * off the contacts-only component.
 */
function storageKey(scope: SearchScope): string {
  return `mv-${scope}-recent-searches:v1`;
}

const RECENT_SEARCHES_MAX = 10;

function readRaw(scope: SearchScope): unknown {
  try {
    const raw = localStorage.getItem(storageKey(scope));
    if (!raw) return null;
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
  try {
    localStorage.setItem(storageKey(scope), JSON.stringify(queries.slice(0, RECENT_SEARCHES_MAX)));
  } catch {
    // Private browsing and full storage can throw.
  }
}

/** Remove every saved query for one search bar. */
export function clearRecentSearches(scope: SearchScope): void {
  try {
    localStorage.removeItem(storageKey(scope));
  } catch {
    // Private browsing and full storage can throw.
  }
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

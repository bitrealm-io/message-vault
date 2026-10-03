/**
 * The search language, as the server describes it.
 *
 * The browser keeps no list of words of its own: `GET /v1/search-fields/{list}`
 * says which words one list accepts, what kind of value each takes, and which values
 * a choice word allows, and the search box builds its suggestions from that.
 * What is left here are two rules about the *shape* of a query — whether it
 * carries a `word:` token at all, and what its plain words are — which the
 * contact and conversation lists use to decide whether they can narrow rows in
 * the browser or must ask the server.
 */

import { keys } from "./queryKeys";
import { useRouteQuery } from "./routeQuery";
import { listSearchFields, type SearchFieldList } from "./serverApi";
import type { components } from "./serverApi.types";

type Schema = components["schemas"];
export type SearchField = Schema["FieldDoc"];
export type SearchList = SearchFieldList;

/**
 * A `word:` token: a field name followed by a colon, at the start or after a
 * space or an opening bracket, with or without a leading minus. Quoted phrases
 * are removed first so a colon inside one does not count.
 */
// The bracket matters because `(kind:group or kind:direct)` is a real query.
// The value must not start with `/`, so a pasted URL like `http://x` is a word.
const FIELD_TOKEN_RE = /(^|[\s(])-?[a-z][a-z-]*:(?!\/)/i;
const PHRASE_RE = /"(?:[^"]|"")*"/g;

/** True when the query has a `word:` token, which only the server can apply. */
export function hasFieldToken(q: string): boolean {
  return FIELD_TOKEN_RE.test(q.replace(PHRASE_RE, " "));
}

/**
 * The field words a query carries, lower-cased and without the leading minus,
 * in order of appearance. Quoted phrases are removed first, as in
 * `hasFieldToken`, so a colon inside one does not read as a word.
 */
export function fieldWords(q: string): string[] {
  const words: string[] = [];
  for (const match of q.replace(PHRASE_RE, " ").matchAll(/(^|[\s(])-?([a-z][a-z-]*):(?!\/)/gi)) {
    const word = match[2].toLowerCase();
    if (!words.includes(word)) words.push(word);
  }
  return words;
}

/**
 * The words in `q` that `fields` (one list's registry) does not carry. A
 * screen that sends one query to two lists uses this to tell which pane a
 * word applies to, instead of letting the other pane answer with a 422.
 */
export function unsupportedFieldWords(q: string, fields: readonly SearchField[]): string[] {
  const known = new Set(fields.map((f) => f.word));
  return fieldWords(q).filter((w) => !known.has(w));
}

/** The free-text words of a query, with every `word:value` token removed. */
export function stripFieldTokens(q: string): string {
  return q
    .replace(/(^|[\s(])-?[a-z][a-z-]*:(?!\/)("(?:[^"]|"")*"|\S*)/gi, " ")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * The words one list accepts, from the server, cached for the session. A `null`
 * list asks the server nothing and has no words.
 *
 * When the request has failed and no words were fetched before, `error` says
 * why and `fields` is empty. A caller that reads `fields` as the list's whole
 * vocabulary checks `error` first, because an empty list refuses every word.
 */
export function useSearchFields(list: SearchList | null): {
  fields: SearchField[];
  loading: boolean;
  error: Error | null;
} {
  const { data, isPending, error } = useRouteQuery(
    keys.searchFields.list(list ?? "conversations"),
    (signal) => listSearchFields(list ?? "conversations", { signal }),
    { staleTime: Number.POSITIVE_INFINITY, enabled: list !== null },
  );
  // A failed refetch keeps the words already fetched, and those still hold.
  return { fields: data ?? [], loading: isPending, error: data === undefined ? error : null };
}

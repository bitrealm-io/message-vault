import { searchTokens } from "./searchQuery";

/** One free-text term of a query: a word or a quoted phrase, and whether it ended in `*`. */
export type FreeTextTerm = { text: string; prefix: boolean };

/** `word:` at the start of a token, as the server's lexer reads a field word. */
const FIELD_HEAD = /^[A-Za-z][A-Za-z-]*:(?!\/)/;

/**
 * The free-text terms a match must or may have, in the order they were
 * typed: every word and quoted phrase that is not a `word:value`, not an
 * operator (`or`, `and`, `not`), and not behind `-` or `not`, alone or in a
 * negated group.
 *
 * The server ranks a Messages search by these same terms
 * (`Expr::positive_text_terms` in `crates/server/server/src/search/parse.rs`),
 * so a query this finds none in is one the server will not sort by
 * relevance. The Messages list draws them in bold.
 */
export function freeTextTerms(query: string): FreeTextTerm[] {
  const terms: FreeTextTerm[] = [];
  /** Whether each open group is negated, innermost last. */
  const groups: boolean[] = [];
  let pendingNot = false;
  for (const token of searchTokens(query)) {
    const raw = query.slice(token.start, token.end);
    const inNegated = groups.at(-1) ?? false;
    if (token.kind === "close") {
      groups.pop();
      continue;
    }
    const minus = raw.startsWith("-") && raw.length > 1;
    const negated = inNegated || pendingNot || minus;
    if (token.kind === "open") {
      groups.push(negated);
      pendingNot = false;
      continue;
    }
    const lower = raw.toLowerCase();
    if (lower === "or" || lower === "and") continue;
    if (lower === "not") {
      pendingNot = true;
      continue;
    }
    pendingNot = false;
    const body = minus ? raw.slice(1) : raw;
    if (negated || FIELD_HEAD.test(body)) continue;
    const term = readTerm(body);
    if (term) terms.push(term);
  }
  return terms;
}

/** True when `query` has a free-text term to rank by. */
export function hasFreeText(query: string): boolean {
  return freeTextTerms(query).length > 0;
}

/** One text token as a term: a quoted phrase unquoted, or a word with its `*` read off. */
function readTerm(body: string): FreeTextTerm | null {
  if (body.startsWith('"')) {
    const inner = body.endsWith('"') && body.length > 1 ? body.slice(1, -1) : body.slice(1);
    const text = inner.replace(/""/g, '"').trim();
    return text ? { text, prefix: false } : null;
  }
  if (body.endsWith("*") && body.length > 1) return { text: body.slice(0, -1), prefix: true };
  return body ? { text: body, prefix: false } : null;
}

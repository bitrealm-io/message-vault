import type { FreeTextTerm } from "./freeTextTerms";

/** A matched span of a string: start inclusive, end exclusive, in UTF-16 units. */
export type MatchRange = [number, number];

/** A letter or a digit: what the full-text index reads as part of a word. */
const WORD_CHAR = "[\\p{L}\\p{N}]";

/**
 * `text` lower-cased with its accents taken off, the way the full-text index
 * (`unicode61 remove_diacritics 2`) reads it, and where each unit of the
 * result came from in `text`, so a match found in the folded form maps back.
 */
function fold(text: string): { folded: string; from: number[]; to: number[] } {
  let folded = "";
  const from: number[] = [];
  const to: number[] = [];
  let i = 0;
  for (const ch of text) {
    const f = ch.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
    for (let k = 0; k < f.length; k++) {
      from.push(i);
      to.push(i + ch.length);
    }
    folded += f;
    i += ch.length;
  }
  return { folded, from, to };
}

/** The words of a term as the index splits them: runs of letters and digits. */
function termWords(text: string): string[] {
  return fold(text).folded.match(/[\p{L}\p{N}]+/gu) ?? [];
}

/** A pattern for one term: its words next to each other, a whole word or a word's start. */
function termPattern(term: FreeTextTerm): string | null {
  const words = termWords(term.text).map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  if (words.length === 0) return null;
  const end = term.prefix ? `${WORD_CHAR}*` : `(?!${WORD_CHAR})`;
  return `(?<!${WORD_CHAR})${words.join(`[^\\p{L}\\p{N}]+`)}${end}`;
}

/**
 * Where `terms` occur in `text`, in order, with ranges that overlap or meet
 * merged. A term matches as the full-text index matches it: a whole word
 * (or the start of one, for a prefix), ignoring case and accents, and a
 * phrase or a word with punctuation in it as its words next to each other.
 */
export function matchRanges(text: string, terms: readonly FreeTextTerm[]): MatchRange[] {
  const patterns = terms.map(termPattern).filter((p): p is string => p !== null);
  if (patterns.length === 0 || !text) return [];
  const { folded, from, to } = fold(text);
  const found: MatchRange[] = [];
  for (const pattern of patterns) {
    for (const m of folded.matchAll(new RegExp(pattern, "gu"))) {
      if (m[0].length === 0) continue;
      const start = m.index;
      found.push([from[start], to[start + m[0].length - 1]]);
    }
  }
  found.sort((a, b) => a[0] - b[0] || b[1] - a[1]);
  const merged: MatchRange[] = [];
  for (const range of found) {
    const last = merged.at(-1);
    if (last && range[0] <= last[1]) last[1] = Math.max(last[1], range[1]);
    else merged.push([range[0], range[1]]);
  }
  return merged;
}

/** Characters kept before the first match when the text is cut. */
const LEAD = 40;

/**
 * `text` cut so its first match shows: from a word about 40 characters
 * before the match, with `…` where the cut was, and the match ranges in the
 * cut text. Text whose first match is near its start, or that has no match,
 * is kept from its start; the end is left for the row to clamp.
 */
export function snippet(
  text: string,
  terms: readonly FreeTextTerm[],
): { text: string; ranges: MatchRange[] } {
  const ranges = matchRanges(text, terms);
  const first = ranges[0]?.[0] ?? 0;
  if (first <= LEAD) return { text, ranges };
  const space = text.slice(first - LEAD, first).search(/\s\S/);
  const cut = space === -1 ? first : first - LEAD + space + 1;
  const shift = cut - 1;
  return {
    text: `…${text.slice(cut)}`,
    ranges: ranges.map(([a, b]) => [a - shift, b - shift]),
  };
}

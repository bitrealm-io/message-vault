/**
 * Every place in the app that composes a server search string, in one leaf
 * module.
 *
 * The server's search language (`crates/server/server/src/search/`) reads a
 * quoted value by scanning to the next unescaped `"`, and a doubled `""`
 * inside a quoted value is one literal quote
 * (`crates/server/server/src/search/lex.rs`, `read_quoted`). There is no
 * backslash escape — `\"` would come through as two literal characters, a
 * backslash and a quote. A value must be quoted whenever it holds
 * whitespace, `(`, or `)`, because the lexer treats an unquoted `(`/`)` as
 * the language's own grouping syntax rather than as text
 * (`crates/server/server/src/search/lex.rs`, `is_bare_end`); a Contact Group
 * named `Family (close)` sent as `group:Family (close)` therefore means
 * something different from what the person picked. An unquoted value is
 * also split on commas, read as an id when it is `#` and digits, and read as
 * a prefix when it ends in `*` (`crates/server/server/src/search/parse.rs`),
 * so a value with a comma, a leading `#`, or a trailing `*` is quoted too.
 *
 * A page's filter and the text the person typed are joined by `narrow`, and
 * by nothing else, so an `or` in the typed text stays inside the filter.
 *
 * This module only turns values into search terms — no fetching, no React,
 * no imports from `components/`.
 */

/** Quote `value` if the language would otherwise read it as syntax rather than one value. */
export function quote(value: string): string {
  if (value === "" || /[\s(),"]/.test(value) || value.startsWith("#") || value.endsWith("*")) {
    return `"${value.replace(/"/g, '""')}"`;
  }
  return value;
}

// --- Reading a query as the server's lexer does ----------------------

/**
 * One token of a query: where it starts and ends, and whether it opens a
 * group (`(`, or `-(` with its minus), closes one, or is anything else.
 */
export type SearchToken = { kind: "open" | "close" | "text"; start: number; end: number };

/** The lexer's whitespace (`u8::is_ascii_whitespace`), plus NUL, which it reads as a space. */
function isSpace(c: string | undefined): boolean {
  return c === " " || c === "\t" || c === "\n" || c === "\f" || c === "\r" || c === "\0";
}

/** A character that ends an unquoted run (`lex.rs`, `is_bare_end`). */
function isBareEnd(c: string | undefined): boolean {
  return isSpace(c) || c === "(" || c === ")";
}

/**
 * The index just past the quoted value whose opening quote is at `quote`.
 * `""` inside is one quote. A quote that never closes runs to the end.
 */
function quotedEnd(query: string, quote: number): number {
  let i = quote + 1;
  while (i < query.length) {
    if (query[i] === '"') {
      if (query[i + 1] === '"') {
        i += 2;
        continue;
      }
      return i + 1;
    }
    i += 1;
  }
  return query.length;
}

/**
 * Split `query` into tokens the way the server's lexer does
 * (`crates/server/server/src/search/lex.rs`, `tokenize`): a leading `-`
 * belongs to the token after it, a quote opens a value only at the start of
 * a token or right after `word:`, and an unquoted run ends at whitespace or a
 * parenthesis. Where the lexer refuses a quote that never closes, this
 * returns a token that runs to the end, so a box still being typed in can be
 * read.
 */
export function searchTokens(query: string): SearchToken[] {
  const tokens: SearchToken[] = [];
  const n = query.length;
  let i = 0;
  for (;;) {
    while (i < n && isSpace(query[i])) i += 1;
    if (i >= n) return tokens;
    const start = i;
    if (query[i] === ")") {
      tokens.push({ kind: "close", start, end: i + 1 });
      i += 1;
      continue;
    }
    // A `-` with nothing, a space, or `)` after it is a word of its own.
    if (query[i] === "-" && i + 1 < n && !isSpace(query[i + 1]) && query[i + 1] !== ")") i += 1;
    let end: number;
    if (query[i] === "(") {
      tokens.push({ kind: "open", start, end: i + 1 });
      i += 1;
      continue;
    }
    if (query[i] === '"') {
      end = quotedEnd(query, i);
    } else {
      let head = i;
      while (head < n && !isBareEnd(query[head]) && query[head] !== ":") head += 1;
      const isField =
        query[head] === ":" &&
        /^[A-Za-z][A-Za-z-]*$/.test(query.slice(i, head)) &&
        query[head + 1] !== "/";
      if (isField && query[head + 1] === '"') {
        end = quotedEnd(query, head + 1);
      } else {
        end = i;
        while (end < n && !isBareEnd(query[end])) end += 1;
      }
    }
    tokens.push({ kind: "text", start, end });
    i = end;
  }
}

/**
 * The token being typed at the end of `query`, and where it starts: the last
 * token when it runs to the end, or nothing after a trailing space.
 */
export function lastToken(query: string): { start: number; text: string } {
  const last = searchTokens(query).at(-1);
  if (last && last.end === query.length) {
    return { start: last.start, text: query.slice(last.start) };
  }
  return { start: query.length, text: "" };
}

/** `query` with the token being typed replaced by `text`, and the rest left as typed. */
export function replaceLastToken(query: string, text: string): string {
  return query.slice(0, lastToken(query).start) + text;
}

/** True when every `)` in `query` closes a `(` before it, and none is left open. */
function parenthesesPair(query: string): boolean {
  let depth = 0;
  for (const token of searchTokens(query)) {
    if (token.kind === "open") depth += 1;
    if (token.kind === "close") {
      depth -= 1;
      if (depth < 0) return false;
    }
  }
  return depth === 0;
}

/**
 * Narrow the text the person typed to a page's filter, as `filter (typed)`.
 *
 * `or` binds looser than a space (`docs/architecture/search.md`), so without
 * the parentheses `trashed:yes gone or name:jane` would be
 * `(trashed:yes gone) or name:jane` and reach rows outside the filter. Typed
 * text whose parentheses do not pair up is joined without them: wrapped,
 * `a) or (b` would close the group early, and unwrapped, the server refuses
 * it.
 */
export function narrow(filter: string, typed: string): string {
  const f = filter.trim();
  const t = typed.trim();
  if (!t) return f;
  if (!f) return t;
  return parenthesesPair(t) ? `${f} (${t})` : `${f} ${t}`;
}

/** A Contact Group term, e.g. `group:Family` or `group:"Family (close)"`. */
export function forGroup(name: string): string {
  return `group:${quote(name)}`;
}

/** A Message Tag term, e.g. `tag:Work` or `tag:"Book Club"`. */
export function forTag(name: string): string {
  return `tag:${quote(name)}`;
}

/** An identity term, e.g. `identity:ann@example.com` or `identity:"Ann Lee"`. */
export function forHandle(handle: string): string {
  return `identity:${quote(handle.trim())}`;
}

/**
 * A Person word pointing at one contact by id, e.g. `with:#42` or `from:#7`.
 *
 * `word` is any of the language's Person words (`with`, `from`, `to` today);
 * callers read it from the field registry rather than hard-coding the set, so
 * a new Person word works without a change here. The id is numeric, so it
 * never needs quoting.
 */
export function forPerson(word: string, id: string): string {
  return `${word}:#${id}`;
}

/** `all`, `direct`, or `group` — the same three ways a set of conversations narrows by kind. */
export type ConversationKind = "all" | "direct" | "group";

/**
 * Narrow `query` to direct or group conversations, or leave it unchanged for
 * `all` rather than adding an empty term.
 */
export function withKind(query: string, kind: ConversationKind): string {
  if (kind === "all") return query;
  return narrow(`kind:${kind}`, query);
}

/** Trash is always `trashed:yes`; a typed search narrows within it. */
export function trashed(search: string): string {
  return narrow("trashed:yes", search);
}

/** One autocomplete term, e.g. `tag:Work` or `tag:"Book Club"`. */
export function suggestion(word: string, value: string): string {
  return `${word}:${quote(value)}`;
}

// --- Advanced search -------------------------------------------------

/** Same operators as web-next CountField (Any / Equal to / More than / Less than). */
export type CountComparator = "=" | ">" | "<";

export type CountFilterInput = {
  comparator: CountComparator | "any";
  value: string;
};

export type ActivityFilter = "any" | "messages" | "no-messages";

/** Operator for the First message / Last message calendar bounds. */
export type DateBoundOp = "any" | "after" | "before" | "between";

export type DateBoundFilter = {
  op: DateBoundOp;
  /** On or after / Before date, or Between start. */
  start: string;
  /** Between end only. */
  end: string;
};

export type MessagesQueryInput = {
  nameOrHandle: string;
  handle: string;
  msgType: ConversationKind;
  participants: CountFilterInput;
  /** Source ids, as an import writes them: `imessage`, `sms-backup-restore`, … */
  sources: readonly (string | number)[];
};

export type ContactsQueryInput = {
  contactName: string;
  handle: string;
  firstMessageBound: DateBoundFilter;
  lastMessageBound: DateBoundFilter;
  activity: ActivityFilter;
  noPreferredName: boolean;
  noHandle: boolean;
  services: readonly (string | number)[];
};

export function composeCountComparison(input: CountFilterInput): string | null {
  if (input.comparator === "any") return null;
  const value = input.value.trim();
  if (!/^\d+$/.test(value)) return null;
  return `${input.comparator}${value}`;
}

/** Push one `prefix:` date token: `>=D`, `<D`, `<=D`, or an inclusive `D..D` range. */
function pushDateBoundTokens(
  push: (s: string) => void,
  prefix: "first-message" | "last-message",
  bound: DateBoundFilter,
): void {
  switch (bound.op) {
    case "any":
      return;
    case "after":
      if (bound.start) push(`${prefix}:>=${bound.start}`);
      return;
    case "before":
      if (bound.start) push(`${prefix}:<${bound.start}`);
      return;
    case "between":
      if (bound.start && bound.end) push(`${prefix}:${bound.start}..${bound.end}`);
      else if (bound.start) push(`${prefix}:>=${bound.start}`);
      // `a..b` takes in all of b, so an end alone takes in its day too.
      else if (bound.end) push(`${prefix}:<=${bound.end}`);
      return;
  }
}

/**
 * Push one Choice word for the ticked values. Several go in one word, comma
 * separated, which the language reads as "any of these".
 */
function pushChoices(
  push: (s: string) => void,
  word: "service" | "source",
  ids: readonly (string | number)[],
): void {
  const values = ids.map((id) => String(id).trim()).filter(Boolean);
  if (values.length > 0) push(`${word}:${values.join(",")}`);
}

/** The Advanced Search messages form, as one search query. */
export function advancedMessages(input: MessagesQueryInput): string {
  const parts: string[] = [];
  const push = (s: string) => {
    if (s.trim()) parts.push(s.trim());
  };
  if (input.nameOrHandle.trim()) push(input.nameOrHandle.trim());
  if (input.handle.trim()) push(forHandle(input.handle));
  if (input.msgType === "direct") push("kind:direct");
  if (input.msgType === "group") push("kind:group");
  const participantCmp = composeCountComparison(input.participants);
  if (participantCmp) push(`participants:${participantCmp}`);
  pushChoices(push, "source", input.sources);
  return parts.join(" ");
}

/** The Advanced Search contacts form, as one search query. */
export function advancedContacts(input: ContactsQueryInput): string {
  const parts: string[] = [];
  const push = (s: string) => {
    if (s.trim()) parts.push(s.trim());
  };
  if (input.contactName.trim()) push(input.contactName.trim());
  if (input.handle.trim()) push(forHandle(input.handle));
  pushDateBoundTokens(push, "first-message", input.firstMessageBound);
  pushDateBoundTokens(push, "last-message", input.lastMessageBound);
  if (input.activity === "messages") push("messages:>0");
  if (input.activity === "no-messages") push("messages:0");
  if (input.noPreferredName) push("name:none");
  if (input.noHandle) push("identity:none");
  pushChoices(push, "service", input.services);
  return parts.join(" ");
}

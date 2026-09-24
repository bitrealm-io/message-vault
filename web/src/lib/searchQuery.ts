/**
 * Every place in the app that composes a vault search string, in one leaf
 * module.
 *
 * The vault's search language (`crates/vault/server/src/search/`) reads a
 * quoted value by scanning to the next unescaped `"`, and a doubled `""`
 * inside a quoted value is one literal quote
 * (`crates/vault/server/src/search/lex.rs`, `read_quoted`). There is no
 * backslash escape — `\"` would come through as two literal characters, a
 * backslash and a quote. A value must be quoted whenever it holds
 * whitespace, `(`, or `)`, because the lexer treats an unquoted `(`/`)` as
 * the language's own grouping syntax rather than as text
 * (`crates/vault/server/src/search/lex.rs`, `is_bare_end`); a Contact Group
 * named `Family (close)` sent as `group:Family (close)` therefore means
 * something different from what the person picked.
 *
 * This module only turns values into search terms — no fetching, no React,
 * no imports from `components/`.
 */

/** Quote `value` if the language would otherwise read it as more than one token. */
export function quote(value: string): string {
  if (value === "" || /[\s()"]/.test(value)) {
    return `"${value.replace(/"/g, '""')}"`;
  }
  return value;
}

/** A Contact Group term, e.g. `group:Family` or `group:"Family (close)"`. */
export function forGroup(name: string): string {
  return `group:${quote(name)}`;
}

/** A Message Tag term, e.g. `tag:Work` or `tag:"Book Club"`. */
export function forTag(name: string): string {
  return `tag:${quote(name)}`;
}

/** A handle term, e.g. `handle:ann@example.com` or `handle:"Ann Lee"`. */
export function forHandle(handle: string): string {
  return `handle:${quote(handle.trim())}`;
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
 * `all` rather than appending an empty term.
 */
export function withKind(query: string, kind: ConversationKind): string {
  if (kind === "all") return query;
  const term = `kind:${kind}`;
  return query ? `${query} ${term}` : term;
}

/** Trash is always `trashed:yes`; a typed search narrows within it. */
export function trashed(search: string): string {
  const term = search.trim();
  return term ? `trashed:yes ${term}` : "trashed:yes";
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

/** Operator for the first-heard/last-heard calendar bounds. */
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
};

export type ContactsQueryInput = {
  contactName: string;
  handle: string;
  firstHeardBound: DateBoundFilter;
  lastHeardBound: DateBoundFilter;
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

/** Push one `prefix:` date token: `>=D`, `<D`, or an inclusive `D..D` range. */
function pushDateBoundTokens(
  push: (s: string) => void,
  prefix: "first-heard" | "last-heard",
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
      else if (bound.end) push(`${prefix}:<${bound.end}`);
      return;
  }
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
  pushDateBoundTokens(push, "first-heard", input.firstHeardBound);
  pushDateBoundTokens(push, "last-heard", input.lastHeardBound);
  if (input.activity === "messages") push("messages:>0");
  if (input.activity === "no-messages") push("messages:0");
  if (input.noPreferredName) push("name:none");
  if (input.noHandle) push("handle:none");
  // Several ticked transports go in one word, comma separated, which the
  // language reads as "any of these".
  const services = input.services.map((id) => String(id).trim()).filter(Boolean);
  if (services.length > 0) push(`service:${services.join(",")}`);
  return parts.join(" ");
}

/**
 * The Advanced Search contacts form as Trash sends it. Trash runs one query
 * on both the Contacts and the Conversations list, and `first-heard:` and
 * `last-heard:` are Contacts words, so the heard dates are left out; the
 * form does not show them in Trash.
 */
export function advancedTrash(input: ContactsQueryInput): string {
  const noBound: DateBoundFilter = { op: "any", start: "", end: "" };
  return advancedContacts({ ...input, firstHeardBound: noBound, lastHeardBound: noBound });
}

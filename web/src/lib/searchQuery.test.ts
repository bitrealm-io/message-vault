import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  advancedContacts,
  advancedMessages,
  type ContactsQueryInput,
  forGroup,
  forHandle,
  forPerson,
  forTag,
  lastToken,
  type MessagesQueryInput,
  narrow,
  quote,
  replaceLastToken,
  searchTokens,
  suggestion,
  trashed,
  withKind,
} from "./searchQuery";

describe("quote", () => {
  it("returns a plain word bare", () => {
    expect(quote("Family")).toBe("Family");
  });

  it("quotes a value with a space", () => {
    expect(quote("Book Club")).toBe('"Book Club"');
  });

  it("quotes a value with parentheses, since the language reads them as grouping", () => {
    expect(quote("Family (close)")).toBe('"Family (close)"');
    expect(quote("(x")).toBe('"(x"');
    expect(quote("x)")).toBe('"x)"');
  });

  it("quotes a value with an embedded quote and escapes it by doubling", () => {
    expect(quote('say "hi"')).toBe('"say ""hi"""');
  });

  it("round-trips an empty string to something the language accepts", () => {
    expect(quote("")).toBe('""');
  });
});

describe("forGroup", () => {
  it("builds a bare group term", () => {
    expect(forGroup("Family")).toBe("group:Family");
  });

  it("quotes a group name that needs it", () => {
    expect(forGroup("Family (close)")).toBe('group:"Family (close)"');
  });
});

describe("forTag", () => {
  it("builds a bare tag term", () => {
    expect(forTag("Work")).toBe("tag:Work");
  });

  it("quotes a tag name that needs it", () => {
    expect(forTag("Book Club")).toBe('tag:"Book Club"');
  });
});

describe("names the server would read as syntax", () => {
  // Unquoted, the server splits a value on commas, reads #N as an id and a
  // trailing * as a prefix (crates/server/server/src/search/parse.rs).
  it("quotes a group name with a comma", () => {
    expect(forGroup("Smith,Jones")).toBe('group:"Smith,Jones"');
  });
  it("quotes a tag name that looks like an id", () => {
    expect(forTag("#3")).toBe('tag:"#3"');
  });
  it("quotes a tag name that ends in a star", () => {
    expect(forTag("Work*")).toBe('tag:"Work*"');
  });
});

describe("forHandle", () => {
  it("trims and builds a bare handle term", () => {
    expect(forHandle("  ann@example.com  ")).toBe("handle:ann@example.com");
  });

  it("quotes a handle with a space", () => {
    expect(forHandle("Ann Lee")).toBe('handle:"Ann Lee"');
  });

  it("produces the same text on a messages screen and a contacts screen", () => {
    const messages = advancedMessages({
      nameOrHandle: "",
      handle: "Ann Lee",
      msgType: "all",
      participants: { comparator: "any", value: "" },
      sources: [],
    });
    const contacts = advancedContacts({
      contactName: "",
      handle: "Ann Lee",
      firstMessageBound: { op: "any", start: "", end: "" },
      lastMessageBound: { op: "any", start: "", end: "" },
      activity: "any",
      noPreferredName: false,
      noHandle: false,
      services: [],
    });
    expect(messages).toBe(forHandle("Ann Lee"));
    expect(contacts).toBe(forHandle("Ann Lee"));
  });
});

describe("forPerson", () => {
  it("builds a word:#id term for each Person word", () => {
    expect(forPerson("with", "42")).toBe("with:#42");
    expect(forPerson("from", "7")).toBe("from:#7");
    expect(forPerson("to", "7")).toBe("to:#7");
  });
});

describe("narrow", () => {
  it("puts the typed text in parentheses after the filter", () => {
    expect(narrow("tag:Work", "a or b")).toBe("tag:Work (a or b)");
  });

  it("is the filter alone when nothing is typed", () => {
    expect(narrow("tag:Work", "   ")).toBe("tag:Work");
  });

  it("is the typed text alone when there is no filter", () => {
    expect(narrow("", "  a or b ")).toBe("a or b");
  });

  it("leaves typed text whose parentheses do not pair up outside the parentheses", () => {
    // Wrapped, `a) or (b` would close the parentheses early and the `or`
    // would reach past the filter. Unwrapped, the server refuses the
    // stray `)`.
    expect(narrow("tag:Work", "a) or (b")).toBe("tag:Work a) or (b");
    expect(narrow("tag:Work", "(a")).toBe("tag:Work (a");
  });

  it("does not count a parenthesis inside quotes", () => {
    expect(narrow("tag:Work", '"a)" or subject:"(b"')).toBe('tag:Work ("a)" or subject:"(b")');
  });
});

describe("withKind", () => {
  it("returns the query unchanged for all, rather than appending an empty term", () => {
    expect(withKind("q", "all")).toBe("q");
    expect(withKind("", "all")).toBe("");
  });

  it("puts the query in parentheses before kind:direct or kind:group", () => {
    expect(withKind("q", "direct")).toBe("kind:direct (q)");
    expect(withKind("a or b", "group")).toBe("kind:group (a or b)");
  });

  it("builds a bare kind term when the query is empty", () => {
    expect(withKind("", "group")).toBe("kind:group");
  });
});

describe("trashed", () => {
  it("is bare trashed:yes with no search", () => {
    expect(trashed("")).toBe("trashed:yes");
    expect(trashed("   ")).toBe("trashed:yes");
  });

  it("puts a trimmed search in parentheses after trashed:yes", () => {
    expect(trashed("  ada  ")).toBe("trashed:yes (ada)");
  });

  it("keeps a typed or inside the Trash", () => {
    expect(trashed("gone or name:jane")).toBe("trashed:yes (gone or name:jane)");
  });
});

// The cases of the server's lexer (crates/server/server/src/search/lex.rs,
// its tests), read for where each token starts and ends.
describe("searchTokens", () => {
  const texts = (q: string) => searchTokens(q).map((t) => q.slice(t.start, t.end));

  it("reads words, phrases and prefixes", () => {
    expect(texts('hello "two words" avoc*')).toEqual(["hello", '"two words"', "avoc*"]);
  });

  it("reads bare and quoted field values", () => {
    expect(texts('tag:Work group:"Book Club" date:2019..2021')).toEqual([
      "tag:Work",
      'group:"Book Club"',
      "date:2019..2021",
    ]);
  });

  it("reads a doubled quote as a quote inside the value", () => {
    expect(texts('title:"say ""hi"" now" x')).toEqual(['title:"say ""hi"" now"', "x"]);
  });

  it("reads operators, negation and parentheses", () => {
    expect(texts("-tag:Work or (a and not b)")).toEqual([
      "-tag:Work",
      "or",
      "(",
      "a",
      "and",
      "not",
      "b",
      ")",
    ]);
    expect(searchTokens("(a)").map((t) => t.kind)).toEqual(["open", "text", "close"]);
  });

  it("reads a minus before a group as part of the group's parenthesis", () => {
    expect(texts("-(a or b)")).toEqual(["-(", "a", "or", "b", ")"]);
    expect(searchTokens("-(a)")[0].kind).toBe("open");
    // A `-` before a space or a closing parenthesis is a word.
    expect(texts("- a")).toEqual(["-", "a"]);
    expect(texts("(a -)")).toEqual(["(", "a", "-", ")"]);
  });

  it("reads a field word only when it is letters and hyphens", () => {
    expect(texts("http://x")).toEqual(["http://x"]);
    expect(texts('First-Message:"a b"')).toEqual(['First-Message:"a b"']);
    // Not a field word, so the quote is text and the space ends the word.
    expect(texts('1x:"a b"')).toEqual(['1x:"a', 'b"']);
  });

  it("reads a quote inside a bare word as text", () => {
    expect(texts('ab"c d')).toEqual(['ab"c', "d"]);
  });

  it("runs a quote that never closes to the end", () => {
    expect(texts('a tag:"Book (Club')).toEqual(["a", 'tag:"Book (Club']);
    expect(texts('"a b')).toEqual(['"a b']);
  });

  it("reads tabs and line breaks as spaces", () => {
    expect(texts("a\tb\nc")).toEqual(["a", "b", "c"]);
  });
});

describe("lastToken", () => {
  it("is the token that runs to the end of the query", () => {
    expect(lastToken("hello wi")).toEqual({ start: 6, text: "wi" });
    expect(lastToken("(ta")).toEqual({ start: 1, text: "ta" });
    expect(lastToken('with:"ann l')).toEqual({ start: 0, text: 'with:"ann l' });
  });

  it("is empty after a space", () => {
    expect(lastToken("hello ")).toEqual({ start: 6, text: "" });
    expect(lastToken("")).toEqual({ start: 0, text: "" });
  });
});

describe("replaceLastToken", () => {
  it("changes only the token being typed", () => {
    expect(replaceLastToken('subject:"a  b"   wi', "with:")).toBe('subject:"a  b"   with:');
    expect(replaceLastToken("(ta", "tag:")).toBe("(tag:");
    expect(replaceLastToken("a ", "tag:")).toBe("a tag:");
  });
});

describe("suggestion", () => {
  it("builds a bare word:value term", () => {
    expect(suggestion("tag", "Work")).toBe("tag:Work");
  });

  it("quotes a value that needs it", () => {
    expect(suggestion("tag", "Book Club")).toBe('tag:"Book Club"');
  });
});

describe("advancedMessages", () => {
  it("joins only the terms the person filled in", () => {
    expect(
      advancedMessages({
        nameOrHandle: "",
        handle: "",
        msgType: "all",
        participants: { comparator: "any", value: "" },
        sources: [],
      }),
    ).toBe("");
  });

  it("builds one term per filled field, in order, quoting the handle", () => {
    expect(
      advancedMessages({
        nameOrHandle: "ada",
        handle: "Ann Lee",
        msgType: "direct",
        participants: { comparator: ">", value: "3" },
        sources: [],
      }),
    ).toBe('ada handle:"Ann Lee" kind:direct participants:>3');
  });

  it("drops a participants comparison that is not a whole number", () => {
    expect(
      advancedMessages({
        nameOrHandle: "",
        handle: "",
        msgType: "all",
        participants: { comparator: ">", value: "abc" },
        sources: [],
      }),
    ).toBe("");
  });
});

describe("advancedContacts", () => {
  it("joins only the terms the person filled in", () => {
    expect(
      advancedContacts({
        contactName: "",
        handle: "",
        firstMessageBound: { op: "any", start: "", end: "" },
        lastMessageBound: { op: "any", start: "", end: "" },
        activity: "any",
        noPreferredName: false,
        noHandle: false,
        services: [],
      }),
    ).toBe("");
  });

  it("builds date-bound, activity, and none terms", () => {
    expect(
      advancedContacts({
        contactName: "ada",
        handle: "",
        firstMessageBound: { op: "after", start: "2020-01-01", end: "" },
        lastMessageBound: { op: "between", start: "2021-01-01", end: "2021-06-01" },
        activity: "messages",
        noPreferredName: true,
        noHandle: false,
        services: ["imessage", "sms"],
      }),
    ).toBe(
      "ada first-message:>=2020-01-01 last-message:2021-01-01..2021-06-01 messages:>0 name:none service:imessage,sms",
    );
  });

  it("keeps the end day of a Between with an end date only, as a start and an end do", () => {
    // `a..b` takes in all of b; `<b` stops before b (search/value.rs, parse_date).
    expect(
      advancedContacts({
        contactName: "",
        handle: "",
        firstMessageBound: { op: "any", start: "", end: "" },
        lastMessageBound: { op: "between", start: "", end: "2021-06-01" },
        activity: "any",
        noPreferredName: false,
        noHandle: false,
        services: [],
      }),
    ).toBe("last-message:<=2021-06-01");
  });

  it("quotes a handle with a space instead of always quoting it", () => {
    expect(
      advancedContacts({
        contactName: "",
        handle: "ann@example.com",
        firstMessageBound: { op: "any", start: "", end: "" },
        lastMessageBound: { op: "any", start: "", end: "" },
        activity: "any",
        noPreferredName: false,
        noHandle: false,
        services: [],
      }),
    ).toBe("handle:ann@example.com");
  });
});

// --- The fixture the server's search tests read -----------------------
//
// This module is the only place the web composes a search query, and the
// server's search language (crates/server/server/src/search/) is the only
// thing that gets to say whether a query is valid. Nothing on this side
// checks that agreement — a builder could emit a query the language refuses
// and nothing here would notice until someone hit it at runtime.
//
// So this test calls every builder with a fixed set of inputs, including
// the awkward ones that produced the quoting bugs this module was written
// to fix (a name with a space, a name with a parenthesis, a name with a
// quote), and writes one line per result to
// tests/fixtures/search/web-queries.txt: the list the query is meant for,
// a tab, then the query text. crates/server/server/src/search/tests.rs reads
// that file back and asserts every line parses on the list its first column
// names.
//
// The committed file is generated, not authored — this test fails when it
// drifts from what the builders produce today, the same way
// scripts/check-generated-api-types.sh fails when serverApi.types.ts drifts
// from the OpenAPI spec.

/** The server's three searchable lists, spelled the way the fixture and the
 * Rust `ListKind` enum both name them. */
type ListName = "contacts" | "conversations" | "messages";

/** Values chosen to be awkward for the quoter: plain, a space, a balanced
 * parenthesis, an embedded quote (escaped by doubling), and a lone
 * unmatched parenthesis. The first four are only ever *silently wrong* when
 * left unquoted — the language still parses "group:Book Club" as two valid
 * clauses, just not the one clause the person meant. The unmatched
 * parenthesis is the one shape here the language actually refuses when
 * unquoted (`Unbalanced`), which is what lets the Rust side of this fixture
 * ever go red for a quoting regression rather than silently accepting a
 * differently-wrong query. */
const AWKWARD_NAMES = [
  "Ana",
  "Book Club",
  "Family (close)",
  'Say "Hi"',
  "x)",
  "Smith,Jones",
  "#3",
  "Work*",
];

function addLines(lines: Set<string>, query: string, lists: readonly ListName[]): void {
  for (const list of lists) lines.add(`${list}\t${query}`);
}

/** Every query every builder in searchQuery.ts can produce, tagged with the
 * list(s) the server's field registry (search/fields.rs) accepts each term
 * on. Sorted so the fixture's diff is stable. */
function buildFixtureLines(): string[] {
  const lines = new Set<string>();
  const everyList: readonly ListName[] = ["contacts", "conversations", "messages"];

  for (const name of AWKWARD_NAMES) {
    addLines(lines, forGroup(name), everyList);
    addLines(lines, forTag(name), everyList);
    addLines(lines, forHandle(name), everyList);
    addLines(lines, suggestion("group", name), everyList);
    addLines(lines, suggestion("tag", name), everyList);
  }

  // forPerson: the search box's contact autocomplete builds one of these for
  // whichever Person word the person typed, so the lists come from the field
  // registry (search/fields.rs): `with` is a Conversations and Messages word,
  // `from` and `to` are Messages only. `with:` is not a Contacts word — a
  // contact can't be "with" itself.
  const personWordLists: Record<string, readonly ListName[]> = {
    with: ["conversations", "messages"],
    from: ["messages"],
    to: ["messages"],
  };
  for (const [word, lists] of Object.entries(personWordLists)) {
    for (const id of ["7", "42"]) {
      addLines(lines, forPerson(word, id), lists);
    }
  }

  // withKind composes a base term (from the contact drawer's "browse
  // conversations" action) with the kind narrower, always onto the
  // conversation list it navigates to.
  for (const kind of ["all", "direct", "group"] as const) {
    addLines(lines, withKind(forPerson("with", "42"), kind), ["conversations"]);
    addLines(lines, withKind(forHandle("Book Club"), kind), ["conversations"]);
  }
  addLines(lines, withKind("", "group"), ["conversations"]);

  // trashed: the search both Trash panes (contacts and conversations) put in
  // parentheses after trashed:yes. The search text itself is the person's
  // own query, already valid syntax, not a raw value this builder quotes.
  for (const search of ["", "  ada  ", '"guacamole night"', "gone or ada"]) {
    addLines(lines, trashed(search), ["contacts", "conversations"]);
  }

  const messagesInputs: MessagesQueryInput[] = [
    {
      nameOrHandle: "",
      handle: "",
      msgType: "all",
      participants: { comparator: "any", value: "" },
      sources: [],
    },
    {
      nameOrHandle: "ada",
      handle: "Ann Lee",
      msgType: "direct",
      participants: { comparator: ">", value: "3" },
      sources: [],
    },
    {
      nameOrHandle: "",
      handle: "Family (close)",
      msgType: "group",
      participants: { comparator: "=", value: "5" },
      sources: [],
    },
    {
      nameOrHandle: "",
      handle: 'Say "Hi"',
      msgType: "all",
      participants: { comparator: "<", value: "2" },
      sources: ["sms-backup-restore"],
    },
    {
      nameOrHandle: "",
      handle: "",
      msgType: "all",
      participants: { comparator: "any", value: "" },
      sources: [
        "imessage",
        "whatsapp",
        "sms-backup-restore",
        "imazing",
        "openextract",
        "go-sms-pro",
        "sms-backup-plus",
      ],
    },
  ];
  // Tagged "conversations", not "messages": the Advanced Search messages form
  // and the messages-mode search bar both run on the Conversations list.
  // AppHeader gives that bar `list: "conversations"`, and AppLayout's
  // handleSearch sends what it builds to `/?q=`, which db/conversations.rs
  // reads as ListKind::Conversations. Every word this form emits today is
  // valid on both lists, so tagging it "messages" would pass while proving
  // nothing; add a Messages-only word to the form (`attachments:>0`,
  // `from:me`) and this fixture is the thing that catches the 422.
  for (const input of messagesInputs) {
    addLines(lines, advancedMessages(input), ["conversations"]);
  }

  const contactsInputs: ContactsQueryInput[] = [
    {
      contactName: "",
      handle: "",
      firstMessageBound: { op: "any", start: "", end: "" },
      lastMessageBound: { op: "any", start: "", end: "" },
      activity: "any",
      noPreferredName: false,
      noHandle: false,
      services: [],
    },
    {
      contactName: "ada",
      handle: "",
      firstMessageBound: { op: "after", start: "2020-01-01", end: "" },
      lastMessageBound: { op: "between", start: "2021-01-01", end: "2021-06-01" },
      activity: "messages",
      noPreferredName: true,
      noHandle: false,
      services: ["imessage", "sms"],
    },
    {
      contactName: "",
      handle: "Book Club",
      firstMessageBound: { op: "before", start: "2022-01-01", end: "" },
      lastMessageBound: { op: "any", start: "", end: "" },
      activity: "no-messages",
      noPreferredName: false,
      noHandle: true,
      services: [],
    },
    {
      contactName: "",
      handle: 'Say "Hi"',
      firstMessageBound: { op: "any", start: "", end: "" },
      lastMessageBound: { op: "any", start: "", end: "" },
      activity: "any",
      noPreferredName: false,
      noHandle: false,
      services: ["whatsapp"],
    },
  ];
  for (const input of contactsInputs) {
    addLines(lines, advancedContacts(input), ["contacts"]);
    // Trash's Advanced Search shows this same form and sends the result,
    // behind trashed:yes, to both the contacts and the conversations list.
    // Every word advancedContacts emits must therefore parse on both; this
    // is the check that catches a conversations-only word (#331) or a
    // contacts-only one (#718).
    addLines(lines, trashed(advancedContacts(input)), ["contacts", "conversations"]);
  }

  return [...lines].sort();
}

const FIXTURE_PATH = fileURLToPath(
  new URL("../../../tests/fixtures/search/web-queries.txt", import.meta.url),
);

describe("the web-queries fixture", () => {
  it("matches what today's builders produce", () => {
    const want = `${buildFixtureLines().join("\n")}\n`;
    if (process.env.UPDATE_FIXTURES) {
      writeFileSync(FIXTURE_PATH, want);
    }
    const have = readFileSync(FIXTURE_PATH, "utf8");
    expect(
      have,
      "tests/fixtures/search/web-queries.txt is out of date with the builders in " +
        "searchQuery.ts.\nRegenerate with: (cd web && UPDATE_FIXTURES=1 npx vitest run " +
        "src/lib/searchQuery.test.ts)",
    ).toBe(want);
  });
});

import { useEffect, useState } from "react";
import { type SearchField, type SearchList, useSearchFields } from "./searchFields";
import { forPerson, suggestion as suggestionTerm } from "./searchQuery";
import { listContacts } from "./vaultApi";

interface ContactName {
  id: string;
  name: string;
}

/** One autocomplete entry: unique id, displayed label, text inserted into the query. */
export interface Suggestion {
  id: string;
  label: string;
  insert: string;
}

/** Words whose value is a person, so contact names are offered. */
function isPersonWord(field: SearchField | undefined): boolean {
  return field?.value_type === "person";
}

/** Autocomplete rows for a search box on one list. */
export function buildSearchSuggestions(args: {
  completingValue: boolean;
  personOp: boolean;
  lastToken: string;
  fields: SearchField[];
  contacts: ContactName[];
}): Suggestion[] {
  const colon = args.lastToken.indexOf(":");
  if (args.completingValue) {
    const word = args.lastToken.slice(0, colon).replace(/^-/, "").toLowerCase();
    const typed = args.lastToken.slice(colon + 1).toLowerCase();
    if (args.personOp) {
      return args.contacts.slice(0, 6).map((c) => ({
        id: c.id,
        label: c.name,
        // #id survives names with spaces and renames.
        insert: `${forPerson(word, c.id)} `,
      }));
    }
    const field = args.fields.find((f) => f.word === word);
    if (!field) return [];
    return field.values
      .filter((v) => v.startsWith(typed))
      .map((v) => {
        // The label is the term itself, so a value that needs quoting shows
        // the quotes the row will actually type.
        const term = suggestionTerm(word, v);
        return { id: term, label: term, insert: `${term} ` };
      });
  }
  if (args.lastToken.length === 0) return [];
  const typed = args.lastToken.replace(/^-/, "").toLowerCase();
  return args.fields
    .filter((f) => f.word.startsWith(typed))
    .map((f) => ({ id: f.word, label: `${f.word}:`, insert: `${f.word}:` }));
}

/** Replace the token being typed with a suggestion's text. */
export function applySuggestionToQuery(value: string, suggestion: Suggestion): string {
  const tokens = value.split(/\s+/);
  tokens.pop();
  return tokens.concat(suggestion.insert).join(" ");
}

/**
 * Word and value autocomplete for a search box. A bare prefix completes to a
 * word the list has; a choice word offers its values; a person word fetches
 * matching contacts and inserts `word:#id`.
 */
export function useSearchSuggestions(value: string, list: SearchList | null): Suggestion[] {
  const { fields } = useSearchFields(list);
  const [contacts, setContacts] = useState<ContactName[]>([]);

  const lastToken = value.split(/\s+/).pop() || "";
  const colonIdx = lastToken.indexOf(":");
  const completingValue = colonIdx !== -1;
  const word = completingValue ? lastToken.slice(0, colonIdx).replace(/^-/, "").toLowerCase() : "";
  const valuePart = completingValue ? lastToken.slice(colonIdx + 1).replace(/^"|"$/g, "") : "";
  const personOp = completingValue && isPersonWord(fields.find((f) => f.word === word));

  useEffect(() => {
    if (!personOp) {
      setContacts([]);
      return;
    }
    const ac = new AbortController();
    const t = window.setTimeout(() => {
      listContacts({ q: valuePart, limit: 20, offset: 0 }, { signal: ac.signal })
        .then((res) => setContacts(res.items.map((c) => ({ id: String(c.id), name: c.name }))))
        .catch(() => {
          if (!ac.signal.aborted) setContacts([]);
        });
    }, 150);
    return () => {
      window.clearTimeout(t);
      ac.abort();
    };
  }, [personOp, valuePart]);

  return buildSearchSuggestions({ completingValue, personOp, lastToken, fields, contacts });
}

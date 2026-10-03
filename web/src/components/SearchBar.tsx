import {
  lazy,
  Suspense,
  useCallback,
  useContext,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";
import {
  ComboBox,
  ComboBoxStateContext,
  Group,
  Input,
  ListBox,
  ListBoxItem,
  Popover,
} from "react-aria-components";
import {
  clearRecentSearches,
  loadRecentSearches,
  pushRecentSearch,
  type SearchScope,
} from "../lib/recentSearches";
import type { SearchList } from "../lib/searchFields";
import { popupShadow } from "../lib/uiStyles";
import { useDismissable } from "../lib/useDismissable";
import {
  applySuggestionToQuery,
  type Suggestion,
  useSearchSuggestions,
} from "../lib/useSearchSuggestions";
import { Z_INLINE_PANEL, Z_POPOVER } from "../lib/zLayers";
import type { AdvancedSearchMode } from "./AdvancedSearchForm";
import PlainButton from "./PlainButton";

// The advanced form pulls in the date picker and calendar, about 150 kB of
// the entry chunk that most visits never open. It loads when the panel does.
const AdvancedSearchForm = lazy(() => import("./AdvancedSearchForm"));

function MagnifyingGlassIcon() {
  return (
    <svg
      aria-hidden
      viewBox="0 0 24 24"
      className="ml-3 size-4 shrink-0 text-muted"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <circle cx="11" cy="11" r="7" />
      <path d="M20 20l-3.5-3.5" />
    </svg>
  );
}

function ClockIcon() {
  return (
    <svg
      aria-hidden
      viewBox="0 0 24 24"
      className="size-3.5 shrink-0 text-muted"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </svg>
  );
}

function SlidersIcon() {
  return (
    <svg
      aria-hidden
      viewBox="0 0 24 24"
      className="size-3.5 shrink-0 text-muted"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3" />
      <path d="M1 14h6M9 8h6M17 16h6" />
    </svg>
  );
}

/** While the advanced panel is open, focusing the box does not open the popdown over it. */
function KeepPopdownClosed({ when }: { when: boolean }) {
  const state = useContext(ComboBoxStateContext);
  const isOpen = state?.isOpen ?? false;
  const close = state?.close;
  useEffect(() => {
    if (when && isOpen) close?.();
  }, [when, isOpen, close]);
  return null;
}

type Option = {
  /** Collection key; React Aria builds each row's DOM id from it and the list's id. */
  id: string;
  label: string;
  kind: "suggestion" | "recent" | "advanced";
  run: () => void;
};

const optionClass =
  "box-border flex w-full cursor-pointer items-center gap-2 px-3 py-2 text-left text-[0.875rem] text-text outline-none data-focused:bg-hover";

/**
 * The one search bar every list screen uses: magnifying glass, a popdown of
 * recent searches with an Advanced search row, and the advanced form inline
 * below the bar. While a token is being typed the same popdown autocompletes
 * the words the server says this list accepts, and their values.
 *
 * The box and popdown are React Aria's `ComboBox`, with the typed text as a
 * custom value: focus stays in the box, the arrow keys move through the rows
 * (`aria-activedescendant`), Enter runs the row, and Escape closes the
 * popdown. Every row is an action, not a value to select: a recent search
 * runs, a suggestion edits the query, and the last row opens Advanced search.
 * Enter with no row active runs the text in the box.
 */
export default function SearchBar({
  value,
  onChange,
  onSubmit,
  scope,
  list,
  placeholder,
  advancedMode,
  onOpenChange,
}: {
  value: string;
  onChange: (v: string) => void;
  /** Runs the search. */
  onSubmit: (q: string) => void;
  /** Which bar this is: picks the recents bucket and the DOM id namespace. */
  scope: SearchScope;
  /** Which list the server should describe the search words of; `null` autocompletes nothing. */
  list: SearchList | null;
  /** Placeholder and accessible name, e.g. "Search contacts". */
  placeholder: string;
  /** Which advanced form to show; `null` offers none. */
  advancedMode: AdvancedSearchMode | null;
  /** True while the popdown or advanced panel is open (for list-column stacking). */
  onOpenChange?: (open: boolean) => void;
}) {
  // The box holds its own text. `value` reaches it later than a key or a
  // paste does (the Messages box's value is the address, which React Router
  // updates as a transition), so a box drawn from `value` alone lost keys and
  // ran Enter on the old text (#1000).
  const [text, setText] = useState(value);
  /** What the box sent to `onChange` and `value` has not echoed back yet. */
  const sentRef = useRef<string[]>([]);
  // `value` replaces the text only when it is not an echo of what the box
  // sent: a Saved Search, Clear, or a route change.
  useEffect(() => {
    const echoed = sentRef.current.indexOf(value);
    if (echoed >= 0) {
      sentRef.current.splice(0, echoed + 1);
      return;
    }
    sentRef.current = [];
    setText(value);
  }, [value]);

  const change = (q: string) => {
    sentRef.current.push(q);
    setText(q);
    onChange(q);
  };

  const [popdownOpen, setPopdownOpen] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [recents, setRecents] = useState(() => loadRecentSearches(scope));
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  /** Set when a row ran during the current key press, so Enter does not also
   * run the typed text. Cleared as each key press starts. */
  const ranRowRef = useRef(false);

  const suggestions = useSearchSuggestions(text, list);

  const notifyOpen = useEffectEvent((open: boolean) => {
    onOpenChange?.(open);
  });

  useEffect(() => {
    notifyOpen(popdownOpen || showAdvanced);
  }, [popdownOpen, showAdvanced]);

  // The advanced panel is not part of the combobox, so it closes on its own.
  const closeAdvanced = useCallback(() => setShowAdvanced(false), []);
  useDismissable(showAdvanced, rootRef, closeAdvanced);

  const applyQuery = (q: string, { save }: { save: boolean }) => {
    change(q);
    onSubmit(q);
    if (save && q.trim()) {
      pushRecentSearch(scope, q);
      setRecents(loadRecentSearches(scope));
    }
    setShowAdvanced(false);
  };

  /** Autocomplete edits the query in place; it never runs the search. */
  const applySuggestion = (s: Suggestion) => {
    change(applySuggestionToQuery(text, s));
    inputRef.current?.focus();
  };

  const openAdvanced = () => {
    setShowAdvanced(true);
  };

  // While a token is being completed the popdown is the autocomplete list;
  // otherwise it is the recent searches followed by the Advanced search row.
  const completing = suggestions.length > 0;
  const options: Option[] = completing
    ? suggestions.map((s) => ({
        id: `suggestion-${s.id}`,
        label: s.label,
        kind: "suggestion",
        run: () => applySuggestion(s),
      }))
    : [
        ...recents.map(
          (q): Option => ({
            id: `recent-${q}`,
            label: q,
            kind: "recent",
            run: () => applyQuery(q, { save: true }),
          }),
        ),
        ...(advancedMode
          ? [
              {
                id: "advanced",
                label: "Advanced search",
                kind: "advanced",
                run: openAdvanced,
              } satisfies Option,
            ]
          : []),
      ];

  return (
    <div ref={rootRef} className="relative">
      <ComboBox
        aria-label={placeholder}
        items={options}
        inputValue={text}
        onInputChange={change}
        allowsCustomValue
        menuTrigger="focus"
        shouldFocusWrap
        // Every row is an action, so nothing is ever the selected value.
        selectedKey={null}
        onOpenChange={(open) => {
          setPopdownOpen(open);
          if (open) setRecents(loadRecentSearches(scope));
        }}
      >
        <KeepPopdownClosed when={showAdvanced} />
        <Group
          // Each key press starts with no row run, so a row a click ran never
          // swallows the next Enter. React Aria runs the active row after this.
          onKeyDownCapture={() => {
            ranRowRef.current = false;
          }}
          className="flex items-center rounded-xl border border-border bg-bg focus-within:border-accent"
        >
          <MagnifyingGlassIcon />
          <Input
            ref={inputRef}
            type="search"
            placeholder={placeholder}
            onKeyDown={(e) => {
              if (e.key === "Escape" && showAdvanced) {
                setShowAdvanced(false);
                return;
              }
              if (e.key !== "Enter") return;
              // React Aria has already run the active row, if there was one.
              if (ranRowRef.current) return;
              applyQuery(text, { save: true });
            }}
            // The bar has a Clear search button of its own, so the one the browser
            // draws inside a search input is hidden; otherwise there are two.
            className="min-w-0 flex-1 border-none bg-transparent px-2 py-2.5 text-[0.875rem] text-text outline-none [&::-webkit-search-cancel-button]:appearance-none"
          />
          {text ? (
            <PlainButton
              aria-label="Clear search"
              // Not React Aria's combobox button: that one would open the popdown.
              slot={null}
              onPress={() => {
                change("");
                onSubmit("");
                inputRef.current?.focus();
              }}
              className="mr-2 cursor-pointer border-none bg-transparent px-1 text-[1rem] leading-none text-muted outline-none hover:text-text focus-visible:ring-2 focus-visible:ring-accent"
            >
              ×
            </PlainButton>
          ) : null}
        </Group>
        <Popover
          hidden={showAdvanced}
          placement="bottom start"
          offset={4}
          data-mc-overlay=""
          className={`w-[var(--trigger-width)] overflow-hidden rounded-md border border-border bg-popover outline-none ${Z_POPOVER} ${popupShadow}`}
        >
          {!completing && recents.length > 0 ? (
            <div className="flex items-center justify-between px-3 pb-1 pt-2">
              <span className="text-[0.688rem] font-semibold uppercase tracking-[0.04em] text-muted">
                Recent searches
              </span>
              <PlainButton
                onPress={() => {
                  clearRecentSearches(scope);
                  setRecents([]);
                  inputRef.current?.focus();
                }}
                className="cursor-pointer border-none bg-transparent text-[0.688rem] text-muted outline-none hover:text-text focus-visible:ring-2 focus-visible:ring-accent"
              >
                Clear all
              </PlainButton>
            </div>
          ) : null}
          <ListBox<Option>
            id={`${scope}-search-popdown`}
            className="max-h-72 overflow-x-hidden overflow-y-auto outline-none"
          >
            {(option) => (
              <ListBoxItem
                id={option.id}
                textValue={option.label}
                onAction={() => {
                  ranRowRef.current = true;
                  option.run();
                }}
                className={`${optionClass} ${
                  option.kind === "advanced" && recents.length > 0
                    ? "mt-0 border-t border-border py-2.5"
                    : option.kind === "advanced"
                      ? "py-2.5"
                      : ""
                }`}
              >
                {option.kind === "recent" ? <ClockIcon /> : null}
                {option.kind === "advanced" ? <SlidersIcon /> : null}
                <span className="min-w-0 truncate">{option.label}</span>
              </ListBoxItem>
            )}
          </ListBox>
        </Popover>
      </ComboBox>

      {showAdvanced && advancedMode ? (
        <div className={`absolute top-full left-0 mt-2 w-full min-w-[300px] ${Z_INLINE_PANEL}`}>
          <Suspense fallback={null}>
            <AdvancedSearchForm
              mode={advancedMode}
              withTail
              onApply={(q) => applyQuery(q, { save: true })}
              onClose={() => setShowAdvanced(false)}
            />
          </Suspense>
        </div>
      ) : null}
    </div>
  );
}

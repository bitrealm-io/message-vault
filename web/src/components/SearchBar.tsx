import { lazy, Suspense, useCallback, useEffect, useEffectEvent, useRef, useState } from "react";
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

// The advanced form pulls in the date picker and calendar, about 150 kB of
// the entry chunk that most visits never open. It loads when the panel does.
const AdvancedSearchForm = lazy(() => import("./AdvancedSearchForm"));

/** Element id for one recent-search row, referenced by `aria-activedescendant`. */
function optionId(scope: SearchScope, query: string): string {
  return `${scope}-search-recent-${encodeURIComponent(query)}`;
}

/** Element id for one autocomplete row. */
function suggestionId(scope: SearchScope, id: string): string {
  return `${scope}-search-suggestion-${encodeURIComponent(id)}`;
}

function advancedOptionId(scope: SearchScope): string {
  return `${scope}-search-advanced`;
}

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

/**
 * The one search bar every list screen uses: magnifying glass, a popdown of
 * recent searches with an Advanced search row, and the advanced form inline
 * below the bar. While a token is being typed the same popdown autocompletes
 * the words the vault says this list accepts, and their values.
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
  /** Which list the vault should describe the search words of; `null` autocompletes nothing. */
  list: SearchList | null;
  /** Placeholder and accessible name, e.g. "Search contacts". */
  placeholder: string;
  /** Which advanced form to show; `null` offers none. */
  advancedMode: AdvancedSearchMode | null;
  /** True while the popdown or advanced panel is open (for list-column stacking). */
  onOpenChange?: (open: boolean) => void;
}) {
  const [popdownOpen, setPopdownOpen] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [recents, setRecents] = useState(() => loadRecentSearches(scope));
  /** Row the arrow keys are on; -1 means the typed text itself. */
  const [activeIndex, setActiveIndex] = useState(-1);
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const suggestions = useSearchSuggestions(value, list);

  const notifyOpen = useEffectEvent((open: boolean) => {
    onOpenChange?.(open);
  });

  useEffect(() => {
    notifyOpen(popdownOpen || showAdvanced);
  }, [popdownOpen, showAdvanced]);

  const dismissAll = useCallback(() => {
    setPopdownOpen(false);
    setShowAdvanced(false);
  }, []);
  useDismissable(popdownOpen || showAdvanced, rootRef, dismissAll);

  const applyQuery = (q: string, { save }: { save: boolean }) => {
    onChange(q);
    onSubmit(q);
    if (save && q.trim()) {
      pushRecentSearch(scope, q);
      setRecents(loadRecentSearches(scope));
    }
    setPopdownOpen(false);
    setShowAdvanced(false);
  };

  /** Autocomplete edits the query in place; it never runs the search. */
  const applySuggestion = (s: Suggestion) => {
    onChange(applySuggestionToQuery(value, s));
    setActiveIndex(-1);
    inputRef.current?.focus();
  };

  const openPopdown = () => {
    if (showAdvanced) return;
    setRecents(loadRecentSearches(scope));
    setPopdownOpen(true);
  };

  const openAdvanced = () => {
    setPopdownOpen(false);
    setShowAdvanced(true);
  };

  /**
   * Rows the arrow keys walk. While a token is being completed the popdown is
   * the autocomplete list; otherwise it is the recent searches followed by the
   * Advanced search row. The input keeps DOM focus throughout and points at the
   * current row with `aria-activedescendant`, which is what `role="combobox"`
   * promises — before this the list was reachable only with a pointer.
   */
  const completing = suggestions.length > 0;
  const options = completing
    ? suggestions.map((s) => ({
        id: suggestionId(scope, s.id),
        run: () => applySuggestion(s),
      }))
    : [
        ...recents.map((q) => ({
          id: optionId(scope, q),
          run: () => applyQuery(q, { save: true }),
        })),
        ...(advancedMode ? [{ id: advancedOptionId(scope), run: openAdvanced }] : []),
      ];
  const active = activeIndex >= 0 && activeIndex < options.length ? options[activeIndex] : null;

  const moveActive = (delta: number) => {
    if (!popdownOpen) {
      openPopdown();
      setActiveIndex(0);
      return;
    }
    const count = options.length;
    if (count === 0) return;
    setActiveIndex((at) => (at < 0 ? (delta > 0 ? 0 : count - 1) : (at + delta + count) % count));
  };

  const popdownId = `${scope}-search-popdown`;

  return (
    <div ref={rootRef} className="relative">
      <div className="flex items-center rounded-xl border border-border bg-bg focus-within:border-accent">
        <MagnifyingGlassIcon />
        <input
          ref={inputRef}
          type="search"
          role="combobox"
          value={value}
          placeholder={placeholder}
          aria-label={placeholder}
          aria-expanded={popdownOpen}
          aria-controls={popdownId}
          aria-autocomplete="list"
          aria-activedescendant={popdownOpen && active ? active.id : undefined}
          onChange={(e) => {
            onChange(e.target.value);
            setActiveIndex(-1);
          }}
          onFocus={openPopdown}
          onClick={openPopdown}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              if (showAdvanced) {
                e.preventDefault();
                setShowAdvanced(false);
                return;
              }
              if (popdownOpen) {
                e.preventDefault();
                setPopdownOpen(false);
                setActiveIndex(-1);
              }
              return;
            }
            if (e.key === "ArrowDown") {
              e.preventDefault();
              moveActive(1);
              return;
            }
            if (e.key === "ArrowUp") {
              e.preventDefault();
              moveActive(-1);
              return;
            }
            if (e.key === "Enter") {
              e.preventDefault();
              // A highlighted row wins; otherwise submit what was typed.
              if (active) active.run();
              else applyQuery(value, { save: true });
            }
          }}
          // The bar has a Clear search button of its own, so the one the browser
          // draws inside a search input is hidden; otherwise there are two.
          className="min-w-0 flex-1 border-none bg-transparent px-2 py-2.5 text-[0.875rem] text-text outline-none [&::-webkit-search-cancel-button]:appearance-none"
        />
        {value ? (
          <button
            type="button"
            aria-label="Clear search"
            onClick={() => {
              onChange("");
              onSubmit("");
              inputRef.current?.focus();
            }}
            className="mr-2 cursor-pointer border-none bg-transparent px-1 text-[1rem] leading-none text-muted hover:text-text"
          >
            ×
          </button>
        ) : null}
      </div>

      {popdownOpen && !showAdvanced && options.length > 0 ? (
        <div
          id={popdownId}
          role="listbox"
          className={`absolute top-full right-0 left-0 mt-1 overflow-hidden rounded-md border border-border bg-popover ${Z_POPOVER} ${popupShadow}`}
        >
          {completing ? (
            <ul className="m-0 max-h-72 list-none overflow-auto p-0">
              {suggestions.map((s, i) => (
                <li key={s.id}>
                  <button
                    type="button"
                    role="option"
                    id={suggestionId(scope, s.id)}
                    aria-selected={activeIndex === i}
                    onMouseEnter={() => setActiveIndex(i)}
                    onClick={() => applySuggestion(s)}
                    className={`flex w-full cursor-pointer items-center gap-2 border-none px-3 py-2 text-left text-[0.875rem] text-text hover:bg-hover ${
                      activeIndex === i ? "bg-hover" : "bg-transparent"
                    }`}
                  >
                    <span className="min-w-0 truncate">{s.label}</span>
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <>
              {recents.length > 0 ? (
                <>
                  <div className="flex items-center justify-between px-3 pb-1 pt-2">
                    <span className="text-[0.688rem] font-semibold uppercase tracking-[0.04em] text-muted">
                      Recent searches
                    </span>
                    <button
                      type="button"
                      onClick={() => {
                        clearRecentSearches(scope);
                        setRecents([]);
                      }}
                      className="cursor-pointer border-none bg-transparent text-[0.688rem] text-muted hover:text-text"
                    >
                      Clear all
                    </button>
                  </div>
                  <ul className="m-0 list-none p-0">
                    {recents.map((q, i) => (
                      <li key={q}>
                        <button
                          type="button"
                          role="option"
                          id={optionId(scope, q)}
                          aria-selected={activeIndex === i}
                          onMouseEnter={() => setActiveIndex(i)}
                          onClick={() => applyQuery(q, { save: true })}
                          className={`flex w-full cursor-pointer items-center gap-2 border-none px-3 py-2 text-left text-[0.875rem] text-text hover:bg-hover ${
                            activeIndex === i ? "bg-hover" : "bg-transparent"
                          }`}
                        >
                          <ClockIcon />
                          <span className="min-w-0 truncate">{q}</span>
                        </button>
                      </li>
                    ))}
                  </ul>
                  <div className="mx-2 border-t border-border" />
                </>
              ) : null}
              {advancedMode ? (
                <button
                  type="button"
                  role="option"
                  id={advancedOptionId(scope)}
                  aria-selected={activeIndex === recents.length}
                  onMouseEnter={() => setActiveIndex(recents.length)}
                  onClick={openAdvanced}
                  className={`flex w-full cursor-pointer items-center gap-2 border-none px-3 py-2.5 text-left text-[0.875rem] text-text hover:bg-hover ${
                    activeIndex === recents.length ? "bg-hover" : "bg-transparent"
                  }`}
                >
                  <SlidersIcon />
                  Advanced search
                </button>
              ) : null}
            </>
          )}
        </div>
      ) : null}

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

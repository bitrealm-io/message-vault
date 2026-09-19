import { useState } from "react";
import type { SearchScope } from "../lib/recentSearches";
import type { SearchList } from "../lib/searchFields";
import type { AdvancedSearchMode } from "./AdvancedSearchForm";
import AppAccountMenu from "./AppAccountMenu";
import { loadWidth } from "./columnResize";
import {
  LEFT_PANEL_DEFAULT_WIDTH,
  LEFT_PANEL_MAX_WIDTH,
  LEFT_PANEL_MIN_WIDTH,
  LEFT_PANEL_STORAGE_KEY,
  LEFT_PANEL_WIDTH_VAR,
} from "./leftPanelWidth";
import SearchBar from "./SearchBar";

/** Which list the header search runs against. */
export type HeaderSearchTarget = "accounts" | "contacts" | "messages" | "trash";

/**
 * Every target uses the same bar; only the wording, the recents bucket, the
 * advanced form, and the list whose words it suggests differ. Trash sends one
 * query to the conversations list and the contacts list at once, so its
 * advanced form offers only the words both accept (`trash` mode); the
 * TrashScreen explains any typed word that one of the two lists refuses.
 */
const SEARCH_TARGETS: Record<
  HeaderSearchTarget,
  {
    scope: SearchScope;
    list: SearchList | null;
    placeholder: string;
    advancedMode: AdvancedSearchMode | null;
  }
> = {
  // Owner Home filters the accounts table by username: no search words, no advanced form.
  accounts: {
    scope: "account",
    list: null,
    placeholder: "Search accounts",
    advancedMode: null,
  },
  contacts: {
    scope: "contact",
    list: "contacts",
    placeholder: "Search contacts",
    advancedMode: "contacts",
  },
  messages: {
    scope: "message",
    list: "conversations",
    placeholder: "Search messages",
    advancedMode: "messages",
  },
  trash: {
    scope: "trash",
    list: "conversations",
    placeholder: "Search Trash",
    advancedMode: "trash",
  },
};

/** Full-width bar: app name on the left, search in the middle, the account button on the far right. */
export default function AppHeader({
  searchQuery,
  searchTarget,
  onSearchChange,
  onSearch,
}: {
  searchQuery: string;
  searchTarget: HeaderSearchTarget;
  onSearchChange: (v: string) => void;
  onSearch: (q: string) => void;
}) {
  const target = SEARCH_TARGETS[searchTarget];
  // Same key as LeftPanel so a stored width does not flash at the default.
  const [brandWidth] = useState(() =>
    loadWidth(
      LEFT_PANEL_STORAGE_KEY,
      LEFT_PANEL_DEFAULT_WIDTH,
      LEFT_PANEL_MIN_WIDTH,
      LEFT_PANEL_MAX_WIDTH,
    ),
  );

  return (
    <header className="relative z-20 flex shrink-0 items-center border-b border-border bg-panel">
      <div
        className="box-border flex h-12 shrink-0 items-center px-3"
        style={{ width: `var(${LEFT_PANEL_WIDTH_VAR}, ${brandWidth}px)` }}
      >
        <span className="text-[0.875rem] font-bold text-text">Message Vault</span>
      </div>
      <div className="flex min-w-0 flex-1 items-center justify-center px-3 py-2">
        <div className="w-full max-w-xl">
          <SearchBar
            key={searchTarget}
            value={searchQuery}
            scope={target.scope}
            list={target.list}
            placeholder={target.placeholder}
            advancedMode={target.advancedMode}
            onChange={onSearchChange}
            onSubmit={onSearch}
          />
        </div>
      </div>
      <div className="flex shrink-0 items-center px-3">
        <AppAccountMenu />
      </div>
    </header>
  );
}

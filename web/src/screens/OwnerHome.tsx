import { useState } from "react";
import { Navigate, useNavigate, useParams, useSearchParams } from "react-router-dom";
import AppHeader from "../components/AppHeader";
import { loadWidth } from "../components/columnResize";
import {
  LEFT_PANEL_DEFAULT_WIDTH,
  LEFT_PANEL_MAX_WIDTH,
  LEFT_PANEL_MIN_WIDTH,
  LEFT_PANEL_STORAGE_KEY,
} from "../components/leftPanelWidth";
import { NAV_LEADING_ROW_CLASS } from "../components/navSectionLayout";
import { useAuth } from "../lib/auth";
import { parseSelectKey } from "../lib/selectKey";
import { OwnerAccountsPanel } from "./owner/OwnerAccountsPanel";
import { VaultSettingsPanel } from "./owner/VaultSettingsPanel";
import SettingsScreen from "./SettingsScreen";

/** What the side panel lists, in its order. */
const SECTIONS = ["dashboard", "settings", "accounts", "activity", "logs"] as const;

const SECTION_LABELS: Record<(typeof SECTIONS)[number], string> = {
  dashboard: "Dashboard",
  settings: "Settings",
  accounts: "User Accounts",
  activity: "Activity",
  logs: "Logs",
};

/** Sections the side panel lists before anything is built behind them. */
const EMPTY_SECTIONS: ReadonlySet<(typeof SECTIONS)[number]> = new Set([
  "dashboard",
  "activity",
  "logs",
]);

function sectionLinkClass(active: boolean): string {
  return `${NAV_LEADING_ROW_CLASS} box-border w-full cursor-pointer rounded border-none px-2 py-1.5 text-left text-[0.875rem] text-text hover:bg-hover ${
    active ? "bg-hover font-semibold" : "bg-transparent font-normal"
  }`;
}

/**
 * Owner Home: where the vault owner lands at sign-in and works from, the way
 * any other account lands in Messages.
 *
 * The frame is the one every account sees: the header with the product name,
 * a search bar and the account button, over a side panel and a content pane.
 * What fills it is the owner's own. The owner has no conversations, no
 * contacts, no import, no export and no trash, so the side panel lists
 * Dashboard, Settings, User Accounts, Activity and Logs, and the search bar
 * filters the accounts table. Dashboard, Activity and Logs show only their
 * name: nothing is built behind them yet.
 *
 * `/owner/accounts/{id}` is one account's Settings, the screen its holder
 * sees, opened from the account's name in the table. The owner's own row
 * opens the owner's own Settings, which is also where the account button's
 * Settings goes. See `docs/adr/0008-the-vault-owner-holds-no-messages.md`.
 */
export default function OwnerHome() {
  const { section: raw, accountId: rawAccountId } = useParams();
  const navigate = useNavigate();
  const { accountId: ownAccountId } = useAuth();
  const [searchParams, setSearchParams] = useSearchParams();
  const section = parseSelectKey(raw ?? null, SECTIONS);
  // The width the message shell's side panel was last dragged to, so the
  // product name sits over a panel of the same width on both screens.
  const [navWidth] = useState(() =>
    loadWidth(
      LEFT_PANEL_STORAGE_KEY,
      LEFT_PANEL_DEFAULT_WIDTH,
      LEFT_PANEL_MIN_WIDTH,
      LEFT_PANEL_MAX_WIDTH,
    ),
  );

  // `/owner` and any unknown section land on User Accounts, and the address
  // bar says so, so a reload comes back to the same place.
  if (!section) {
    return <Navigate to="/owner/accounts" replace />;
  }

  // An id that is not a number names no account; the list is the way back.
  const openAccountId =
    section === "accounts" && rawAccountId && /^\d+$/.test(rawAccountId)
      ? Number(rawAccountId)
      : null;
  if (rawAccountId && openAccountId === null) {
    return <Navigate to="/owner/accounts" replace />;
  }

  const accountSearch = searchParams.get("q") || "";

  // The bar searches the accounts table, so typing anywhere else goes to it.
  const handleSearchChange = (q: string) => {
    if (section !== "accounts" || openAccountId !== null) {
      navigate(`/owner/accounts${q ? `?q=${encodeURIComponent(q)}` : ""}`);
      return;
    }
    // `replace`, so typing does not fill the history with one entry per keystroke.
    setSearchParams(q ? { q } : {}, { replace: true });
  };

  return (
    <div className="flex h-screen flex-col bg-bg font-sans text-text">
      <AppHeader
        searchQuery={accountSearch}
        searchTarget="accounts"
        onSearchChange={handleSearchChange}
        onSearch={handleSearchChange}
      />
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <nav
          aria-label="Owner Home sections"
          className="flex h-full shrink-0 flex-col gap-0.5 overflow-auto border-r border-border bg-panel px-3 py-2"
          style={{ width: navWidth }}
        >
          {SECTIONS.map((id) => (
            <button
              key={id}
              type="button"
              aria-current={id === section ? "page" : undefined}
              className={sectionLinkClass(id === section)}
              onClick={() => navigate(`/owner/${id}`)}
            >
              {SECTION_LABELS[id]}
            </button>
          ))}
        </nav>

        <main className="min-w-0 flex-1 overflow-auto bg-bg text-text">
          {openAccountId !== null ? (
            // The owner's own row is the owner's own Settings, not a managed account's.
            <SettingsScreen
              key={openAccountId}
              managedAccountId={openAccountId === ownAccountId ? undefined : openAccountId}
            />
          ) : (
            <div className="max-w-[900px] p-6">
              {EMPTY_SECTIONS.has(section) && (
                <h3 className="m-0 text-text">{SECTION_LABELS[section]}</h3>
              )}
              {section === "settings" && <VaultSettingsPanel />}
              {section === "accounts" && <OwnerAccountsPanel filter={accountSearch} />}
            </div>
          )}
        </main>
      </div>
    </div>
  );
}

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
import { parseSelectKey } from "../lib/selectKey";
import { OwnerAccountsPanel } from "./owner/OwnerAccountsPanel";
import { VaultSettingsPanel } from "./owner/VaultSettingsPanel";
import { AppearanceSection } from "./settings/AppearanceSection";
import { ChangePasswordSection } from "./settings/ChangePasswordSection";

/** What the side panel lists, in its order. */
const NAV_SECTIONS = ["vault", "accounts"] as const;
/** Settings is a section too, reached from the account button the way /settings is. */
const SECTIONS = [...NAV_SECTIONS, "settings"] as const;

const SECTION_LABELS: Record<(typeof NAV_SECTIONS)[number], string> = {
  vault: "Vault Settings",
  accounts: "User Accounts",
};

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
 * contacts, no import, no export and no trash, so the side panel lists Vault
 * Settings and User Accounts, and the search bar filters the accounts table.
 *
 * The owner's Settings, behind the account button, are a password and an
 * appearance and nothing else — no profile, no time zone, no vault. See
 * `docs/adr/0008-the-vault-owner-holds-no-messages.md`.
 */
export default function OwnerHome() {
  const { section: raw } = useParams();
  const navigate = useNavigate();
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

  const accountSearch = searchParams.get("q") || "";

  // The bar searches accounts from any section, so typing elsewhere goes there.
  const handleSearchChange = (q: string) => {
    if (section !== "accounts") {
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
          {NAV_SECTIONS.map((id) => (
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
          <div className="mx-auto max-w-[900px] p-6">
            {section === "vault" && <VaultSettingsPanel />}
            {section === "accounts" && <OwnerAccountsPanel filter={accountSearch} />}
            {section === "settings" && (
              <div className="flex flex-col gap-8">
                <ChangePasswordSection canReset={false} />
                <AppearanceSection />
              </div>
            )}
          </div>
        </main>
      </div>
    </div>
  );
}

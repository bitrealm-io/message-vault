import { SelectionIndicator, Tab, TabList, TabPanel, Tabs } from "react-aria-components";
import { useSearchParams } from "react-router-dom";
import { useAuth } from "../lib/auth";
import { parseSelectKey } from "../lib/selectKey";
import { OwnerAccountsPanel } from "./owner/OwnerAccountsPanel";
import { VaultSettingsPanel } from "./owner/VaultSettingsPanel";
import { AppearanceSection } from "./settings/AppearanceSection";
import { ChangePasswordSection } from "./settings/ChangePasswordSection";

const ALL_TABS = ["accounts", "vault", "password", "appearance"] as const;
type ConsoleTab = (typeof ALL_TABS)[number];

const TAB_LABELS: Record<ConsoleTab, string> = {
  accounts: "User Accounts",
  vault: "Vault",
  password: "Password",
  appearance: "Appearance",
};

function tabFromSearchParam(raw: string | null): ConsoleTab {
  return parseSelectKey(raw, ALL_TABS) ?? "accounts";
}

function tabClassName({ isSelected }: { isSelected: boolean }) {
  return `relative -mb-px cursor-pointer border-none bg-transparent px-3 py-2 text-[0.813rem] font-medium outline-none transition-colors duration-200 focus-visible:ring-2 focus-visible:ring-accent ${
    isSelected ? "text-text" : "text-muted hover:text-text"
  }`;
}

/**
 * Where the vault owner works.
 *
 * The owner has no conversations, no contacts, no import, no export and no
 * trash, so the message-browsing shell means nothing to them: there is no
 * sidebar here and no route into one. Managing accounts is not something the
 * owner adjusts on the side, which is why this is a console of its own rather
 * than a tab inside Settings.
 *
 * The tab named **Password** rather than Account is the whole of what the
 * owner has of their own — no profile, no time zone, no vault. See
 * `docs/adr/0008-the-vault-owner-holds-no-messages.md`.
 */
export default function OwnerConsole() {
  const [searchParams, setSearchParams] = useSearchParams();
  const { logout } = useAuth();
  const tab = tabFromSearchParam(searchParams.get("tab"));

  return (
    <div className="min-h-screen bg-bg">
      <div className="mx-auto max-w-[900px] p-6 text-text">
        <header className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h1 className="m-0 text-[1.375rem] font-semibold tracking-[-0.015em] text-text">
              Message Vault
            </h1>
            <p className="mt-[0.35rem] text-[0.875rem] text-muted">
              You are the owner of this vault. You manage who may use it, and you read no messages.
            </p>
          </div>
          <button
            type="button"
            onClick={() => void logout()}
            className="cursor-pointer rounded border border-border bg-transparent px-3 py-1.5 text-[0.813rem] text-muted transition-colors hover:text-text"
          >
            Sign out
          </button>
        </header>

        <Tabs
          selectedKey={tab}
          onSelectionChange={(key) => {
            const next = parseSelectKey(key, ALL_TABS);
            if (!next) return;
            const params = new URLSearchParams(searchParams);
            params.set("tab", next);
            setSearchParams(params, { replace: true });
          }}
        >
          <TabList
            aria-label="Vault owner sections"
            className="relative mt-5 flex gap-1 border-b border-border"
          >
            {ALL_TABS.map((id) => (
              <Tab key={id} id={id} className={tabClassName}>
                {TAB_LABELS[id]}
                <SelectionIndicator className="absolute bottom-0 left-2 right-2 h-[2px] rounded-full bg-accent transition-[translate,width] duration-200 motion-reduce:transition-none" />
              </Tab>
            ))}
          </TabList>

          <TabPanel id="accounts" className="mt-6">
            <OwnerAccountsPanel />
          </TabPanel>
          <TabPanel id="vault" className="mt-6">
            <VaultSettingsPanel />
          </TabPanel>
          <TabPanel id="password" className="mt-6">
            <ChangePasswordSection />
          </TabPanel>
          <TabPanel id="appearance" className="mt-6">
            <AppearanceSection />
          </TabPanel>
        </Tabs>
      </div>
    </div>
  );
}

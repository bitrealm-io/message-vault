import { SelectionIndicator, Tab, TabList, TabPanel, Tabs } from "react-aria-components";
import { Link, useSearchParams } from "react-router-dom";
import { canUseConvert } from "../lib/desktopFeatures";
import { parseSelectKey } from "../lib/selectKey";
import { isTauri } from "../lib/tauri-check";
import { useSettingsAccount } from "../lib/useSettingsAccount";
import { AccountSettingsPanel } from "./settings/AccountSettingsPanel";
import { AppearanceSection } from "./settings/AppearanceSection";
import { ConvertSection } from "./settings/ConvertSection";
import { ManagedProfilePanel } from "./settings/ManagedProfilePanel";
import { NewAccountPanel } from "./settings/NewAccountPanel";
import { ProfileSettingsPanel } from "./settings/ProfileSettingsPanel";
import { StorageSection } from "./settings/StorageSection";
import { SystemSection } from "./settings/SystemSection";

const ALL_TABS = ["account", "profile", "storage", "system", "convert", "appearance"] as const;
type SettingsTab = (typeof ALL_TABS)[number];

const TAB_LABELS: Record<SettingsTab, string> = {
  account: "Account",
  profile: "Profile",
  storage: "Storage",
  system: "System",
  convert: "Convert",
  appearance: "Appearance",
};

/** System, Convert and Appearance are this device's, not an account's. */
const DEVICE_TABS: readonly SettingsTab[] = ["system", "convert", "appearance"];

/** Tabs about messages, which the vault owner does not hold. */
const OWNER_HIDDEN_TABS: readonly SettingsTab[] = ["storage", "system", "convert"];

/**
 * Tabs this person can open, in display order.
 *
 * - Convert is a desktop-only tool: it runs `message-reexport` in the desktop
 *   process, so a browser visiting the website never sees it.
 * - An account the vault owner opened from User Accounts has the tabs that
 *   are the account's. The device tabs would change the owner's own browser,
 *   so they are in the owner's own Settings only.
 * - The vault owner holds no messages, so its own Settings have no Storage,
 *   and none of the tools that work on messages: System and Convert.
 */
function visibleTabs(isDesktop: boolean, managed: boolean, isOwner: boolean): SettingsTab[] {
  return ALL_TABS.filter((id) => {
    if (managed && DEVICE_TABS.includes(id)) return false;
    if (isOwner && !managed && OWNER_HIDDEN_TABS.includes(id)) return false;
    if (id === "convert") return canUseConvert(isDesktop);
    return true;
  });
}

/** "account, profile, and appearance" — the header sentence built from the visible tabs. */
function tabSummary(tabs: SettingsTab[]): string {
  const names = tabs.map((id) => TAB_LABELS[id].toLowerCase());
  if (names.length <= 1) return names.join("");
  return `${names.slice(0, -1).join(", ")}, and ${names[names.length - 1]}`;
}

function tabFromSearchParam(raw: string | null, allowed: readonly SettingsTab[]): SettingsTab {
  return parseSelectKey(raw, allowed) ?? "account";
}

function tabClassName({ isSelected, isDisabled }: { isSelected: boolean; isDisabled: boolean }) {
  const tone = isDisabled
    ? "cursor-default text-muted opacity-50"
    : `cursor-pointer ${isSelected ? "text-text" : "text-muted hover:text-text"}`;
  return `relative -mb-px border-none bg-transparent px-3 py-2 text-[0.813rem] font-medium outline-none transition-colors duration-200 focus-visible:ring-2 focus-visible:ring-accent ${tone}`;
}

/** A new account has an Account section to fill in; the rest waits for the account. */
const NEW_ACCOUNT_DISABLED_TABS: readonly SettingsTab[] = ["profile", "storage"];

/**
 * Settings for the logged-in account, or, given `managedAccountId`, for an
 * account the vault owner opened from User Accounts. The same screen and the
 * same tabs either way, so the owner sees an account's settings laid out as
 * the account holder does.
 *
 * Given `creating`, the account is one the owner is adding. It has the tabs a
 * managed account has, so the owner sees what the account will hold, but only
 * Account opens: a profile and storage belong to an account that exists.
 * Creating it opens that account's Settings, with every tab.
 *
 * Given `backToAccounts`, the screen was opened from User Accounts and carries
 * a link back to it above the heading, which says whose Settings these are.
 * Owner Home sets it for every account it opens, the owner's own included.
 */
export default function SettingsScreen({
  managedAccountId,
  creating = false,
  backToAccounts = false,
}: {
  managedAccountId?: number;
  creating?: boolean;
  backToAccounts?: boolean;
}) {
  const [searchParams, setSearchParams] = useSearchParams();
  const { profile } = useSettingsAccount(managedAccountId);
  const managed = managedAccountId !== undefined;
  const tabs = creating
    ? visibleTabs(isTauri(), true, false)
    : visibleTabs(isTauri(), managed, profile?.is_owner === true);
  const tab = creating ? "account" : tabFromSearchParam(searchParams.get("tab"), tabs);
  const whose = managed && profile ? `${profile.username}'s` : "your";

  return (
    <div className="max-w-[820px] p-6 text-text">
      <header>
        {backToAccounts ? (
          <Link
            to="/owner/accounts"
            className="mb-2 inline-block text-[0.813rem] text-muted no-underline hover:text-text"
          >
            ← User Accounts
          </Link>
        ) : null}
        <h2 className="m-0 text-text">
          {creating
            ? "New account"
            : managed && profile
              ? `Settings for ${profile.username}`
              : backToAccounts
                ? "Settings for Vault Owner"
                : "Settings"}
        </h2>
        <p className="mt-[0.35rem] text-[0.875rem] text-muted">
          {creating
            ? "Profile and Storage open once the account is created."
            : `Manage ${whose} ${tabSummary(tabs)}.`}
        </p>
      </header>

      <Tabs
        disabledKeys={creating ? NEW_ACCOUNT_DISABLED_TABS : undefined}
        selectedKey={tab}
        onSelectionChange={(key) => {
          const next = parseSelectKey(key, tabs);
          if (!next) return;
          const params = new URLSearchParams(searchParams);
          params.set("tab", next);
          setSearchParams(params, { replace: true });
        }}
      >
        <TabList
          aria-label="Settings sections"
          className="relative mt-5 flex gap-1 border-b border-border"
        >
          {tabs.map((id) => (
            <Tab key={id} id={id} className={tabClassName}>
              {TAB_LABELS[id]}
              <SelectionIndicator className="absolute bottom-0 left-2 right-2 h-[2px] rounded-full bg-accent transition-[translate,width] duration-200 motion-reduce:transition-none" />
            </Tab>
          ))}
        </TabList>

        <TabPanel id="account" className="mt-6">
          {creating ? (
            <NewAccountPanel />
          ) : (
            <AccountSettingsPanel managedAccountId={managedAccountId} />
          )}
        </TabPanel>
        <TabPanel id="profile" className="mt-6">
          {managed ? (
            <ManagedProfilePanel accountId={managedAccountId} />
          ) : (
            <ProfileSettingsPanel />
          )}
        </TabPanel>
        {tabs.includes("storage") ? (
          <TabPanel id="storage" className="mt-6">
            <StorageSection managedAccountId={managedAccountId} />
          </TabPanel>
        ) : null}
        {tabs.includes("system") ? (
          <TabPanel id="system" className="mt-6">
            <SystemSection />
          </TabPanel>
        ) : null}
        {tabs.includes("convert") ? (
          <TabPanel id="convert" className="mt-6">
            <ConvertSection />
          </TabPanel>
        ) : null}
        {tabs.includes("appearance") ? (
          <TabPanel id="appearance" className="mt-6">
            <AppearanceSection />
          </TabPanel>
        ) : null}
      </Tabs>
    </div>
  );
}

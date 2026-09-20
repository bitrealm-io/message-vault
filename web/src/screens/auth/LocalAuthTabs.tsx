import { SelectionIndicator, Tab, TabList, TabPanel, Tabs } from "react-aria-components";
import { setBaseUrl } from "../../lib/api";
import { useAuth } from "../../lib/auth";
import type { VaultState } from "../../lib/useVaultState";
import ClaimVaultForm from "./ClaimVaultForm";
import CreateAccountForm, { type CreatedAccount } from "./CreateAccountForm";
import LoginForm from "./LoginForm";

function tabClassName({ isSelected }: { isSelected: boolean }) {
  return `relative -mb-px flex-1 cursor-pointer border-none bg-transparent px-3 py-2 text-center text-[0.875rem] font-medium outline-none transition-colors duration-200 focus-visible:ring-2 focus-visible:ring-accent ${
    isSelected ? "text-text" : "text-muted hover:text-text"
  }`;
}

/**
 * The ways into a vault, which depend on what state the vault is in.
 *
 * An **unclaimed** vault offers one thing: creating its owner. No login,
 * because no account exists to log into, and no Create Account, because a
 * vault decides who may join it only once it has an owner to decide.
 *
 * A **closed** vault offers Login alone. An **open** one adds Create Account.
 *
 * The vault reports which of the three it is; nothing here recombines the
 * facts behind that answer. See
 * `docs/adr/0008-the-vault-owner-holds-no-messages.md`.
 *
 * Each panel keeps its own busy and error state, so switching tabs leaves the
 * other form's message behind.
 */
export default function LocalAuthTabs({
  serverUrl,
  vaultState,
  disabled = false,
}: {
  serverUrl: string;
  vaultState: VaultState;
  disabled?: boolean;
}) {
  const { login } = useAuth();

  const logInToNewAccount = async (created: CreatedAccount) => {
    // A stranger's registration opens a Session on the new account; the token
    // is absent only when the owner created it, which this screen never does.
    if (!created.token) {
      throw new Error("The vault created the account but opened no session.");
    }
    // Awaited so the empty-profile check inside `login` runs before the form
    // drops its busy state, sending the new account on to profile setup.
    await login(serverUrl.trim(), created.token, created.account_id);
  };

  // One thing to do, so no tab strip to choose between things.
  if (vaultState === "unclaimed") {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <h2 className="mb-6 border-b border-border pb-2 text-center text-[0.875rem] font-medium text-text">
          Create Vault Owner
        </h2>
        <ClaimVaultForm serverUrl={serverUrl} disabled={disabled} />
      </div>
    );
  }

  if (vaultState === "closed") {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <h2 className="mb-6 border-b border-border pb-2 text-center text-[0.875rem] font-medium text-text">
          Login
        </h2>
        <LoginForm serverUrl={serverUrl} disabled={disabled} />
      </div>
    );
  }

  return (
    <Tabs defaultSelectedKey="login" className="flex min-h-0 flex-1 flex-col">
      <TabList
        aria-label="Log in or create an account"
        className="relative mb-6 flex border-b border-border"
      >
        <Tab id="login" className={tabClassName}>
          Login
          <SelectionIndicator className="absolute bottom-0 left-2 right-2 h-[2px] rounded-full bg-accent transition-[translate,width] duration-200 motion-reduce:transition-none" />
        </Tab>
        <Tab id="create" className={tabClassName}>
          Create Account
          <SelectionIndicator className="absolute bottom-0 left-2 right-2 h-[2px] rounded-full bg-accent transition-[translate,width] duration-200 motion-reduce:transition-none" />
        </Tab>
      </TabList>

      <TabPanel id="login" className="flex min-h-0 flex-1 flex-col outline-none">
        <LoginForm serverUrl={serverUrl} disabled={disabled} />
      </TabPanel>
      <TabPanel id="create" className="flex min-h-0 flex-1 flex-col outline-none">
        {/* "Continue", not "Create account": this step opens the account but
            does not finish it — the profile setup screen it leads to does. */}
        <CreateAccountForm
          submitLabel="Continue"
          busyLabel="Continuing…"
          onBeforeCreate={() => setBaseUrl(serverUrl.trim())}
          onCreated={logInToNewAccount}
          disabled={disabled}
        />
      </TabPanel>
    </Tabs>
  );
}

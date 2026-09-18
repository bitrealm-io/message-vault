import { Navigate, useNavigate, useParams } from "react-router-dom";
import { NAV_LEADING_ROW_CLASS } from "../components/navSectionLayout";
import { useAuth } from "../lib/auth";
import { parseSelectKey } from "../lib/selectKey";
import { OwnerAccountsPanel } from "./owner/OwnerAccountsPanel";
import { VaultSettingsPanel } from "./owner/VaultSettingsPanel";
import { AppearanceSection } from "./settings/AppearanceSection";
import { ChangePasswordSection } from "./settings/ChangePasswordSection";

const SECTIONS = ["accounts", "vault", "password", "appearance"] as const;
type Section = (typeof SECTIONS)[number];

const SECTION_LABELS: Record<Section, string> = {
  accounts: "User Accounts",
  vault: "Vault",
  password: "Password",
  appearance: "Appearance",
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
 * The owner has no conversations, no contacts, no import, no export and no
 * trash, so the message-browsing shell means nothing to them. This screen has
 * a side panel of its own instead, with User Accounts first because managing
 * accounts is what the owner is for.
 *
 * The entry named **Password** rather than Account is the whole of what the
 * owner has of their own — no profile, no time zone, no vault. See
 * `docs/adr/0008-the-vault-owner-holds-no-messages.md`.
 */
export default function OwnerHome() {
  const { section: raw } = useParams();
  const navigate = useNavigate();
  const { logout } = useAuth();
  const section = parseSelectKey(raw ?? null, SECTIONS);

  // `/owner` and any unknown section land on User Accounts, and the address
  // bar says so, so a reload comes back to the same place.
  if (!section) {
    return <Navigate to="/owner/accounts" replace />;
  }

  return (
    <div className="flex min-h-screen bg-bg text-text">
      <nav
        aria-label="Owner Home sections"
        className="flex w-[220px] shrink-0 flex-col border-r border-border bg-panel"
      >
        <div className="border-b border-border px-4 py-3">
          <h1 className="m-0 text-[1.125rem] font-semibold tracking-[-0.015em] text-text">
            Message Vault
          </h1>
          <p className="mt-1 text-[0.75rem] text-muted">Vault owner</p>
        </div>
        <div className="flex flex-col gap-0.5 px-2 py-2">
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
        </div>
      </nav>

      <div className="min-w-0 flex-1 overflow-auto">
        <div className="mx-auto max-w-[900px] p-6">
          <header className="flex flex-wrap items-start justify-between gap-3">
            <p className="m-0 text-[0.875rem] text-muted">
              You are the owner of this vault. You manage who may use it, and you read no messages.
            </p>
            <button
              type="button"
              onClick={() => void logout()}
              className="cursor-pointer rounded border border-border bg-transparent px-3 py-1.5 text-[0.813rem] text-muted transition-colors hover:text-text"
            >
              Sign out
            </button>
          </header>

          <div className="mt-6">
            {section === "accounts" && <OwnerAccountsPanel />}
            {section === "vault" && <VaultSettingsPanel />}
            {section === "password" && <ChangePasswordSection />}
            {section === "appearance" && <AppearanceSection />}
          </div>
        </div>
      </div>
    </div>
  );
}

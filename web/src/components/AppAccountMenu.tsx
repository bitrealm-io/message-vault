import { useCallback, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { useAuth } from "../lib/auth";
import { useAccountProfile } from "../lib/useAccountProfile";
import { useIsVaultOwner } from "../lib/useIsVaultOwner";
import { Z_POPOVER } from "../lib/zLayers";
import { GearIcon, PersonIcon, SignOutIcon } from "./icons";
import PopupMenu from "./PopupMenu";

const itemRow = "flex items-center gap-2";

/**
 * The circle user button at the far right of the header. Its menu names who is
 * signed in (username, then preferred name when one is set) and opens Settings
 * or logs out.
 */
export default function AppAccountMenu() {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const navigate = useNavigate();
  const location = useLocation();
  const { logout } = useAuth();
  const { profile } = useAccountProfile();
  // The owner's settings are a section of Owner Home; every other account has /settings.
  const { isOwner } = useIsVaultOwner();
  const settingsPath = isOwner ? "/owner/settings" : "/settings";
  const settingsActive = location.pathname.startsWith(settingsPath);

  const close = useCallback(() => setOpen(false), []);

  const username = profile?.username ?? "";
  const preferredName = profile?.preferred_name?.trim() ?? "";

  return (
    <div className="relative">
      <button
        type="button"
        ref={triggerRef}
        aria-expanded={open}
        aria-haspopup="menu"
        aria-label="Account menu"
        title={username || undefined}
        onClick={() => setOpen((v) => !v)}
        className={`flex h-8 w-8 cursor-pointer items-center justify-center rounded-full border border-border bg-transparent p-0 text-text hover:bg-hover ${
          open ? "bg-hover" : ""
        }`}
      >
        <PersonIcon size={18} />
      </button>
      <PopupMenu
        open={open}
        onClose={close}
        triggerRef={triggerRef}
        label="Account menu"
        className={`absolute top-full right-0 mt-1 min-w-[12rem] rounded-xl ${Z_POPOVER}`}
        header={
          username ? (
            <div className="flex flex-col gap-0.5">
              <span className="font-semibold" data-testid="account-menu-username">
                {username}
              </span>
              {preferredName ? (
                <span className="text-muted" data-testid="account-menu-preferred-name">
                  {preferredName}
                </span>
              ) : null}
            </div>
          ) : null
        }
        items={[
          {
            label: "Settings",
            onSelect: () => navigate(settingsPath),
            children: (
              <span className={`${itemRow} ${settingsActive ? "font-semibold" : ""}`}>
                <GearIcon size={15} />
                Settings
              </span>
            ),
          },
          {
            label: "Log out",
            danger: true,
            onSelect: () => logout(),
            children: (
              <span className={itemRow}>
                <SignOutIcon size={15} />
                Log out
              </span>
            ),
          },
        ]}
      />
    </div>
  );
}

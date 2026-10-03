import { useLocation, useNavigate } from "react-router-dom";
import { useAuth } from "../lib/auth";
import { useAccountProfile } from "../lib/useAccountProfile";
import { useIsOwner } from "../lib/useIsOwner";
import { GearIcon, LogOutIcon, PersonIcon } from "./icons";
import PlainButton from "./PlainButton";
import PopupMenu from "./PopupMenu";

const itemRow = "flex items-center gap-2";

/**
 * The logged-in account's username and the circle user button at the far right
 * of the header. The username is always on screen, for every account, so
 * nobody has to open the menu to learn whose messages these are. The menu
 * names the account too (username, then preferred name when one is set) and
 * opens Settings or logs out.
 */
export default function AppAccountMenu() {
  const navigate = useNavigate();
  const location = useLocation();
  const { logout, accountId } = useAuth();
  const { profile } = useAccountProfile();
  // The owner's Settings are its own row in User Accounts; every other account has /settings.
  const { isOwner } = useIsOwner();
  const settingsPath = isOwner ? `/owner/accounts/${accountId}` : "/settings";
  const settingsActive = location.pathname.startsWith(settingsPath);

  const username = profile?.username ?? "";
  const preferredName = profile?.preferred_name?.trim() ?? "";

  return (
    <div className="relative flex items-center gap-2">
      {username ? (
        // A long username is cut short, and less of it shows as the window narrows, so the search bar keeps its room.
        <span
          className="max-w-[6rem] truncate text-[0.813rem] text-text md:max-w-[10rem] lg:max-w-[14rem]"
          title={username}
          data-testid="header-username"
        >
          {username}
        </span>
      ) : null}
      <PopupMenu
        trigger={
          <PlainButton
            aria-label="Account menu"
            title={username || undefined}
            className="flex h-8 w-8 cursor-pointer items-center justify-center rounded-full border border-border bg-transparent p-0 text-text outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-accent aria-expanded:bg-hover"
          >
            <PersonIcon size={18} />
          </PlainButton>
        }
        label="Account menu"
        className="min-w-[12rem] rounded-xl"
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
                <LogOutIcon size={15} />
                Log out
              </span>
            ),
          },
        ]}
      />
    </div>
  );
}

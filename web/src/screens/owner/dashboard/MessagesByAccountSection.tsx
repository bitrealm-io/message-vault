import ScrollingTableCard from "../../../components/ScrollingTableCard";
import { tdClass, tdMuted } from "../../settings/apiTokensUtils";
import { formatBytes } from "../../settings/storage/storageUtils";
import { DashboardSection } from "./DashboardSection";
import type { VaultStorage } from "./types";

/** A column heading: bold, in the text color, as the User Accounts table has it. */
const thClass = "px-3 py-2 text-left text-[0.75rem] font-bold text-text";

/** The line between one column heading and the next. */
const thSeparator = "border-l border-border";

/** A figure lines up on the right, so sizes can be read down the column. */
const numberCell = "whitespace-nowrap text-right";

/** Every other row is a shade lighter, the way the User Accounts table is striped. */
const rowStripe = "even:[&>td]:bg-hover/50";

/**
 * One row per account: its username, how many messages it holds, how much
 * text they are, and the estimated share of the messages' storage. The
 * accounts come in the order the User Accounts table lists them: the owner
 * first, then by username. The totals row at the bottom carries the
 * whole-vault figures, so the split can be seen to add up.
 *
 * The estimate is the measured messages-on-disk figure split by each
 * account's share of text; the hint says so.
 */
export function MessagesByAccountSection({ storage }: { storage: VaultStorage }) {
  const totalText = storage.accounts.reduce((sum, account) => sum + account.text_bytes, 0);
  return (
    <DashboardSection
      title="Messages by account"
      hint="Estimated size on disk is the messages-on-disk figure split by each account's share of text."
    >
      <ScrollingTableCard cardClassName="rounded-xl bg-elevated">
        <table className="w-full border-collapse" aria-label="Messages by account">
          <thead>
            <tr>
              <th className={thClass}>Account</th>
              <th className={`${thClass} ${thSeparator} text-right`}>Messages</th>
              <th className={`${thClass} ${thSeparator} text-right`}>Text</th>
              <th className={`${thClass} ${thSeparator} text-right`}>Estimated size on disk</th>
            </tr>
          </thead>
          <tbody>
            {storage.accounts.map((account) => (
              <tr key={account.account_id} className={`border-t border-border ${rowStripe}`}>
                <td className={`${tdClass} whitespace-nowrap font-semibold`}>{account.username}</td>
                <td className={`${tdMuted} ${numberCell}`}>
                  {account.message_count.toLocaleString()}
                </td>
                <td className={`${tdMuted} ${numberCell}`}>{formatBytes(account.text_bytes)}</td>
                <td className={`${tdClass} ${numberCell}`}>
                  {formatBytes(account.estimated_message_bytes)}
                </td>
              </tr>
            ))}
            <tr className="border-t border-border">
              <td className={`${tdClass} whitespace-nowrap font-semibold`}>Whole vault</td>
              <td className={`${tdClass} ${numberCell} font-semibold`}>
                {storage.message_count.toLocaleString()}
              </td>
              <td className={`${tdClass} ${numberCell} font-semibold`}>{formatBytes(totalText)}</td>
              <td className={`${tdClass} ${numberCell} font-semibold`}>
                {formatBytes(storage.messages_bytes)}
              </td>
            </tr>
          </tbody>
        </table>
      </ScrollingTableCard>
    </DashboardSection>
  );
}

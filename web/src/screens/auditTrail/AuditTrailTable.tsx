import ScrollingTableCard from "../../components/ScrollingTableCard";
import { type AuditEntry, auditActorLabel, describeAuditEntry } from "../../lib/auditTrail";
import { formatDateTime } from "../../lib/formatDate";
import PageControl from "../settings/storage/PageControl";
import { sectionHint, tableCard, tdStyle, thStyle } from "../settings/storage/storageUtils";
import { AUDIT_TRAIL_PAGE_SIZE } from "./useAuditTrail";

/** The Account column: the username, marked when the account has since been deleted. */
function accountCell(entry: AuditEntry) {
  // Opening and closing the Message Crate to new accounts is about no account.
  if (!entry.username) return <span className="text-muted">—</span>;
  // A refused login for an unknown username never had an account to lose.
  const deleted = entry.account_id == null && entry.reason !== "unknown_username";
  return (
    <>
      {entry.username}
      {deleted ? <span className="block text-[0.75rem] text-muted">deleted</span> : null}
    </>
  );
}

/**
 * One page of an Audit Trail, newest first: when, what, and who acted, with
 * the account each entry is about when the table lists more than one
 * account. Every entry says what happened and how much, never what a message
 * said.
 */
export default function AuditTrailTable({
  entries,
  total,
  page,
  onPageChange,
  showAccount,
}: {
  /** The entries on this page. */
  entries: AuditEntry[];
  /** How many entries the trail holds, across every page. */
  total: number;
  page: number;
  onPageChange: (page: number) => void;
  /** Show the Account column: the owner's list of every account. */
  showAccount: boolean;
}) {
  if (total === 0) {
    return <p className={`${sectionHint} mt-3`}>Nothing recorded yet.</p>;
  }
  return (
    <div className="mt-3 flex flex-col gap-3">
      <ScrollingTableCard cardClassName={tableCard}>
        <table className="w-full border-collapse">
          <thead>
            <tr>
              <th className={thStyle}>Date</th>
              {showAccount ? <th className={thStyle}>Account</th> : null}
              <th className={thStyle}>What happened</th>
              <th className={thStyle}>By</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((entry) => (
              <tr key={`${entry.action}-${entry.id}`}>
                <td className={`${tdStyle} whitespace-nowrap`}>{formatDateTime(entry.at)}</td>
                {showAccount ? <td className={tdStyle}>{accountCell(entry)}</td> : null}
                <td className={tdStyle}>
                  {/* The card sizes to its content, so a long line wraps here
                      rather than pushing the By column out of view. */}
                  <div className="max-w-[30rem]">{describeAuditEntry(entry)}</div>
                </td>
                <td className={`${tdStyle} whitespace-nowrap`}>{auditActorLabel(entry.actor)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </ScrollingTableCard>
      <PageControl
        page={page}
        total={total}
        pageSize={AUDIT_TRAIL_PAGE_SIZE}
        onPageChange={onPageChange}
      />
    </div>
  );
}

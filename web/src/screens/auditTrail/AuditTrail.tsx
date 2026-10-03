import { sectionHint } from "../settings/storage/storageUtils";
import AuditTrailTable from "./AuditTrailTable";
import { type AuditTrailOf, useAuditTrail } from "./useAuditTrail";

/**
 * One Audit Trail, read and shown: a loading line, the error, or the table
 * with its page control. Owner Home and Settings both show their trails
 * through this, so the two read and say the same thing.
 */
export default function AuditTrail({
  of,
  showAccount,
}: {
  /** Whose Audit Trail to read. */
  of: AuditTrailOf;
  /** Show the Account column: the owner's list of every account. */
  showAccount: boolean;
}) {
  const trail = useAuditTrail(of);
  if (trail.loading) return <p className={`${sectionHint} mt-3`}>Loading the Audit Trail…</p>;
  if (trail.error) return <p className="mt-3 text-[0.875rem] text-danger">{trail.error}</p>;
  return (
    <AuditTrailTable
      entries={trail.entries}
      total={trail.total}
      page={trail.page}
      onPageChange={trail.setPage}
      showAccount={showAccount}
    />
  );
}

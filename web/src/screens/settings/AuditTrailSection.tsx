import AuditTrailTable from "../auditTrail/AuditTrailTable";
import { useAuditTrail } from "../auditTrail/useAuditTrail";
import { sectionHint, sectionTitle } from "./storage/storageUtils";

/**
 * The account's own Audit Trail, under Settings: every entry about this
 * account, whoever acted, so its holder reads when the owner changed it as
 * well as their own logins, imports and exports. Given `managedAccountId`,
 * the owner reads the same entries for the account they opened.
 */
export function AuditTrailSection({ managedAccountId }: { managedAccountId?: number }) {
  const trail = useAuditTrail(
    managedAccountId === undefined ? { kind: "own" } : { kind: "account", id: managedAccountId },
  );

  return (
    <section>
      <h3 className={sectionTitle}>Audit Trail</h3>
      <p className={sectionHint}>
        What was done with this account and when: logins, imports, exports, and changes made by the
        account holder or the owner.
      </p>
      {trail.loading ? (
        <p className={`${sectionHint} mt-3`}>Loading…</p>
      ) : trail.error ? (
        <p className="mt-3 text-[0.875rem] text-danger">{trail.error}</p>
      ) : (
        <AuditTrailTable
          entries={trail.entries}
          total={trail.total}
          page={trail.page}
          onPageChange={trail.setPage}
          showAccount={false}
        />
      )}
    </section>
  );
}

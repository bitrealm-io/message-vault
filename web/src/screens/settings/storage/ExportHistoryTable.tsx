import type { ExportRow } from "./storageUtils";
import {
  describeExportScope,
  formatBytes,
  formatImportDate,
  sectionHint,
  sectionTitle,
  tableWrap,
  tdStyle,
  thStyle,
} from "./storageUtils";

/** The word the table shows for a run's status. */
function statusLabel(status: string): string {
  switch (status) {
    case "running":
      return "Running";
    case "completed":
      return "Completed";
    case "failed":
      return "Failed";
    case "cancelled":
      return "Cancelled";
    default:
      return status;
  }
}

/**
 * Every Export Run recorded for the account: what was asked for and how much
 * matched, never what the messages said. A run that never finished shows how
 * far it got in the Delivered column.
 */
export default function ExportHistoryTable({ exports }: { exports: ExportRow[] }) {
  return (
    <section>
      <h3 className={sectionTitle}>Export history</h3>
      <p className={sectionHint}>Each export recorded for this account.</p>
      {exports.length === 0 ? (
        <p className={`${sectionHint} mt-3`}>No exports recorded yet.</p>
      ) : (
        <div className={`${tableWrap} mt-3`}>
          <table className="w-full border-collapse">
            <thead>
              <tr>
                <th className={thStyle}>Date</th>
                <th className={thStyle}>Scope</th>
                <th className={thStyle}>Status</th>
                <th className={`${thStyle} text-right`}>Messages</th>
                <th className={`${thStyle} text-right`}>Delivered</th>
                <th className={`${thStyle} text-right`}>Attachments</th>
                <th className={`${thStyle} text-right`}>Size</th>
              </tr>
            </thead>
            <tbody>
              {exports.map((row) => (
                <tr key={row.id}>
                  <td className={tdStyle}>{formatImportDate(row.finished_at ?? row.started_at)}</td>
                  <td className={tdStyle}>{describeExportScope(row.scope)}</td>
                  <td className={tdStyle}>{statusLabel(row.status)}</td>
                  <td className={`${tdStyle} text-right tabular-nums`}>
                    {row.message_count.toLocaleString()}
                  </td>
                  <td className={`${tdStyle} text-right tabular-nums`}>
                    {row.messages_delivered.toLocaleString()}
                  </td>
                  <td className={`${tdStyle} text-right tabular-nums`}>
                    {row.attachment_count.toLocaleString()}
                  </td>
                  <td className={`${tdStyle} text-right tabular-nums`}>
                    {formatBytes(row.total_bytes)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

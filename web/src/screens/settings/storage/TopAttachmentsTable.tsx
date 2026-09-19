import Button from "../../../components/Button";
import ScrollingTableCard from "../../../components/ScrollingTableCard";
import type { TopAttachment } from "./storageUtils";
import {
  ATTACHMENT_PAGE_SIZE,
  formatBytes,
  sectionHint,
  sectionTitle,
  tableCard,
  tdStyle,
  thStyle,
} from "./storageUtils";

export default function TopAttachmentsTable({
  topAttachments,
  page,
  onPageChange,
}: {
  topAttachments: TopAttachment[];
  page: number;
  onPageChange: (page: number) => void;
}) {
  const pageCount = Math.max(1, Math.ceil(topAttachments.length / ATTACHMENT_PAGE_SIZE));
  const pageRows = topAttachments.slice(
    page * ATTACHMENT_PAGE_SIZE,
    page * ATTACHMENT_PAGE_SIZE + ATTACHMENT_PAGE_SIZE,
  );

  return (
    <section>
      <h3 className={sectionTitle}>Largest attachments</h3>
      <p className={sectionHint}>
        Top {topAttachments.length || 100} attachments by file size
        {topAttachments.length > ATTACHMENT_PAGE_SIZE ? ` · ${ATTACHMENT_PAGE_SIZE} per page` : ""}.
      </p>
      {topAttachments.length === 0 ? (
        <p className={`${sectionHint} mt-3`}>No attachments with sizes yet.</p>
      ) : (
        <div className="mt-3 flex flex-col gap-3">
          <ScrollingTableCard cardClassName={tableCard}>
            <table className="w-full border-collapse">
              <thead>
                <tr>
                  <th className={thStyle}>Name</th>
                  <th className={thStyle}>Conversation</th>
                  <th className={`${thStyle} text-right`}>Size</th>
                </tr>
              </thead>
              <tbody>
                {pageRows.map((row) => (
                  <tr key={row.id}>
                    <td className={`${tdStyle} max-w-[14rem] truncate`}>
                      {row.original_name || row.mime_type || `Attachment ${row.id}`}
                    </td>
                    <td className={tdStyle}>{row.conversation_title || row.chat_identifier}</td>
                    <td className={`${tdStyle} text-right tabular-nums`}>
                      {formatBytes(row.size_bytes)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </ScrollingTableCard>
          {topAttachments.length > ATTACHMENT_PAGE_SIZE && (
            <div className="flex items-center justify-between gap-3">
              <span className="text-[0.75rem] text-muted">
                Page {page + 1} of {pageCount}
              </span>
              <div className="flex gap-2">
                <Button
                  disabled={page <= 0}
                  onClick={() => onPageChange(Math.max(0, page - 1))}
                  size="sm"
                >
                  Back
                </Button>
                <Button
                  disabled={page >= pageCount - 1}
                  onClick={() => onPageChange(Math.min(pageCount - 1, page + 1))}
                  size="sm"
                >
                  Next
                </Button>
              </div>
            </div>
          )}
        </div>
      )}
    </section>
  );
}

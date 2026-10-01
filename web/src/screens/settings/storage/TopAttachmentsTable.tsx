import ScrollingTableCard from "../../../components/ScrollingTableCard";
import PageControl from "./PageControl";
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
  showConversation,
}: {
  topAttachments: TopAttachment[];
  page: number;
  onPageChange: (page: number) => void;
  /** False for the owner, whom the server does not tell which conversation a file is in. */
  showConversation: boolean;
}) {
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
                  {showConversation ? <th className={thStyle}>Conversation</th> : null}
                  <th className={`${thStyle} text-right`}>Size</th>
                </tr>
              </thead>
              <tbody>
                {pageRows.map((row) => (
                  <tr key={row.id}>
                    <td className={`${tdStyle} max-w-[14rem] truncate`}>
                      {row.original_name || row.mime_type || `Attachment ${row.id}`}
                    </td>
                    {showConversation ? (
                      <td className={tdStyle}>{row.conversation_title || row.chat_identifier}</td>
                    ) : null}
                    <td className={`${tdStyle} text-right tabular-nums`}>
                      {formatBytes(row.size_bytes)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </ScrollingTableCard>
          <PageControl
            page={page}
            total={topAttachments.length}
            pageSize={ATTACHMENT_PAGE_SIZE}
            onPageChange={onPageChange}
          />
        </div>
      )}
    </section>
  );
}

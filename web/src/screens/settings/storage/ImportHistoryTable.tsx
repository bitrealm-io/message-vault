import { Fragment } from "react";
import type { ImportSummaryView } from "../../../components/import/ImportSummaryPanel";
import ScrollingTableCard from "../../../components/ScrollingTableCard";
import ImportDetailPanel from "./ImportDetailPanel";
import type { ImportDetailResponse, ImportRow } from "./storageUtils";
import {
  formatBytes,
  formatImportDate,
  sectionHint,
  sectionTitle,
  tableCard,
  tdStyle,
  thStyle,
} from "./storageUtils";

export default function ImportHistoryTable({
  imports,
  selectedImportId,
  selectedImport,
  selectedImportSummary,
  selectedImportLoading,
  selectedImportError,
  onToggle,
  onCloseDetail,
}: {
  imports: ImportRow[];
  selectedImportId: number | null;
  selectedImport: ImportDetailResponse | null;
  selectedImportSummary: ImportSummaryView | null;
  selectedImportLoading: boolean;
  selectedImportError: string;
  onToggle: (importId: number) => void;
  onCloseDetail: () => void;
}) {
  return (
    <section>
      <h3 className={sectionTitle}>Import history</h3>
      <p className={sectionHint}>Each vault push or CLI import recorded for this account.</p>
      {imports.length === 0 ? (
        <p className={`${sectionHint} mt-3`}>No imports recorded yet.</p>
      ) : (
        <ScrollingTableCard className="mt-3" cardClassName={tableCard}>
          <table className="w-full border-collapse">
            <thead>
              <tr>
                <th className={thStyle}>Date</th>
                <th className={thStyle}>Import type</th>
                <th className={`${thStyle} text-right`}>Messages</th>
                <th className={`${thStyle} text-right`}>Attachments</th>
                <th className={`${thStyle} text-right`}>Uploaded size</th>
              </tr>
            </thead>
            <tbody>
              {imports.map((row) => {
                const isSelected = selectedImportId === row.id;
                const detailId = `import-detail-${row.id}`;
                return (
                  <Fragment key={row.id}>
                    <tr
                      className={`cursor-pointer ${isSelected ? "bg-hover" : "hover:bg-hover"}`}
                      onClick={() => onToggle(row.id)}
                    >
                      <td className={tdStyle}>
                        <button
                          type="button"
                          aria-expanded={isSelected}
                          aria-controls={detailId}
                          onClick={(event) => {
                            event.stopPropagation();
                            onToggle(row.id);
                          }}
                          className="w-full rounded-sm text-left outline-none focus-visible:ring-2 focus-visible:ring-accent"
                        >
                          {formatImportDate(row.finished_at ?? row.started_at)}
                        </button>
                      </td>
                      <td className={tdStyle}>{row.source}</td>
                      <td className={`${tdStyle} text-right tabular-nums`}>
                        {row.message_count.toLocaleString()}
                      </td>
                      <td className={`${tdStyle} text-right tabular-nums`}>
                        {row.attachment_count.toLocaleString()}
                      </td>
                      <td className={`${tdStyle} text-right tabular-nums`}>
                        {formatBytes(row.bytes_uploaded)}
                      </td>
                    </tr>
                    {isSelected ? (
                      <tr>
                        <td colSpan={5} className="border-b border-border p-0">
                          <ImportDetailPanel
                            detailId={detailId}
                            selectedImport={selectedImport}
                            selectedImportSummary={selectedImportSummary}
                            selectedImportLoading={selectedImportLoading}
                            selectedImportError={selectedImportError}
                            onClose={onCloseDetail}
                          />
                        </td>
                      </tr>
                    ) : null}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        </ScrollingTableCard>
      )}
    </section>
  );
}

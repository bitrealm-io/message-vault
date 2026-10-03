import { Fragment } from "react";
import type { ImportSummaryView } from "../../../components/import/ImportSummaryPanel";
import PlainButton from "../../../components/PlainButton";
import ScrollingTableCard from "../../../components/ScrollingTableCard";
import ImportDetailPanel from "./ImportDetailPanel";
import PageControl from "./PageControl";
import type { AccountImportRun, ListedImportRun } from "./storageUtils";
import {
  formatBytes,
  formatImportDate,
  RUN_PAGE_SIZE,
  sectionHint,
  sectionTitle,
  tableCard,
  tdStyle,
  thStyle,
} from "./storageUtils";

export default function ImportHistoryTable({
  imports,
  total,
  page,
  onPageChange,
  selectedImportId,
  selectedImport,
  selectedImportSummary,
  selectedImportLoading,
  selectedImportError,
  listContacts,
  onToggle,
  onCloseDetail,
}: {
  /** The runs on this page. */
  imports: ListedImportRun[];
  /** How many runs the account has, across every page. */
  total: number;
  page: number;
  onPageChange: (page: number) => void;
  listContacts: boolean;
  selectedImportId: number | null;
  selectedImport: AccountImportRun | null;
  selectedImportSummary: ImportSummaryView | null;
  selectedImportLoading: boolean;
  selectedImportError: string;
  onToggle: (importId: number) => void;
  onCloseDetail: () => void;
}) {
  return (
    <section>
      <h3 className={sectionTitle}>Import history</h3>
      <p className={sectionHint}>
        Each import recorded for this account, from the desktop app or the command line
        {total > RUN_PAGE_SIZE ? ` · ${RUN_PAGE_SIZE} per page` : ""}.
      </p>
      {total === 0 ? (
        <p className={`${sectionHint} mt-3`}>No imports recorded yet.</p>
      ) : (
        <div className="mt-3 flex flex-col gap-3">
          <ScrollingTableCard cardClassName={tableCard}>
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
                          <PlainButton
                            aria-expanded={isSelected}
                            aria-controls={detailId}
                            // React Aria stops the press here, so the row's own click does not toggle it back.
                            onPress={() => onToggle(row.id)}
                            className="w-full rounded-sm text-left outline-none focus-visible:ring-2 focus-visible:ring-accent"
                          >
                            {formatImportDate(row.finished_at ?? row.started_at)}
                          </PlainButton>
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
                              listContacts={listContacts}
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
          <PageControl
            page={page}
            total={total}
            pageSize={RUN_PAGE_SIZE}
            onPageChange={onPageChange}
          />
        </div>
      )}
    </section>
  );
}

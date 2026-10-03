import { useMemo, useState } from "react";
import {
  Cell,
  Column,
  type Key,
  Button as RACButton,
  Row,
  Table,
  TableBody,
  TableHeader,
  TableLayout,
  Virtualizer,
} from "react-aria-components";
import { ChevronDownIcon, ChevronRightIcon } from "../icons";
import { groupImportIssues, type ImportIssueGroup } from "./groupImportIssues";
import type { ImportIssue } from "./ImportSummaryPanel";
import {
  COLLAPSED_ROW_HEIGHT,
  FILENAME_ROW_PX,
  HEADER_ROW_HEIGHT,
  MAX_VISIBLE_FILENAMES,
  tableViewportHeight,
} from "./importIssuesTableLayout";

/**
 * The Stage (CONTEXT.md) each reported step belongs to. Reading the backup,
 * copying its attachments and writing the conversation files are all
 * Staging from the person's side. A step this build does not know shows as
 * it arrived.
 */
const STAGE_FOR_STEP: Record<string, string> = {
  setup: "Staging",
  parse: "Staging",
  attachments: "Staging",
  prepare: "Staging",
  media: "Media",
  upload: "Upload",
};

const headerClass = "min-w-0 px-3 py-2 text-left font-medium text-muted outline-none";
const cellClass = "min-w-0 overflow-hidden px-3 py-2 text-text";

/** Row height is collapsed until a row expands; React Aria measures that one row. */
const LAYOUT_OPTIONS = {
  estimatedRowHeight: COLLAPSED_ROW_HEIGHT,
  headingHeight: HEADER_ROW_HEIGHT,
};

type IssueRow = ImportIssueGroup & { id: string };

function parseFileLabel(group: ImportIssueGroup): string {
  if (group.items.length === 1) {
    return group.items[0] ?? "";
  }
  return `${group.items.length} files`;
}

function rowAriaLabel(group: ImportIssueGroup, expanded: boolean): string {
  const verb = expanded ? "Collapse" : "Expand";
  return `${verb} error for ${parseFileLabel(group)}`;
}

/**
 * The errors of an import, one row per distinct error. React Aria's virtualized
 * `Table` draws only the rows in view, moves focus between rows with the arrow
 * keys, and runs the row's action on a click or Enter, which expands the row to
 * the whole error and, for a group, its file names.
 */
export default function VirtualizedImportIssuesTable({ issues }: { issues: ImportIssue[] }) {
  const rows = useMemo<IssueRow[]>(
    () => groupImportIssues(issues).map((group, index) => ({ ...group, id: String(index) })),
    [issues],
  );
  const [expandedKey, setExpandedKey] = useState<Key | null>(null);
  const expandedRow = rows.find((row) => row.id === expandedKey) ?? null;
  const viewportHeight = tableViewportHeight(
    rows.length,
    expandedRow == null
      ? null
      : { reason: expandedRow.reason, fileCount: expandedRow.items.length },
  );

  const toggleRow = (key: Key) => {
    setExpandedKey((current) => (current === key ? null : key));
  };

  return (
    <div className="mt-2 w-full min-w-0 max-w-full overflow-hidden rounded-lg border border-border text-left text-[0.813rem]">
      <Virtualizer layout={TableLayout} layoutOptions={LAYOUT_OPTIONS}>
        <Table
          aria-label="Import errors"
          onRowAction={toggleRow}
          className="block w-full overflow-x-hidden overflow-y-auto outline-none"
          style={{ height: HEADER_ROW_HEIGHT + viewportHeight }}
        >
          <TableHeader className="border-b border-border bg-elevated">
            <Column id="file" isRowHeader width="1fr" className={headerClass}>
              Parse File
            </Column>
            <Column id="stage" width={72} className={headerClass}>
              Stage
            </Column>
            <Column id="reason" width="1.4fr" className={headerClass}>
              Error Message
            </Column>
          </TableHeader>
          <TableBody items={rows} dependencies={[expandedKey]}>
            {(row) => {
              const expanded = row.id === expandedKey;
              const fileLabel = parseFileLabel(row);
              return (
                <Row
                  id={row.id}
                  textValue={fileLabel}
                  className={`cursor-pointer items-start border-b border-border outline-none hover:bg-hover focus-visible:bg-hover focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent ${
                    expanded ? "bg-hover" : ""
                  }`}
                >
                  <Cell className={cellClass}>
                    <span className="flex min-w-0 items-start gap-1">
                      {/* The row is the press target; this button says the row expands, and whether it is. */}
                      <RACButton
                        aria-label={rowAriaLabel(row, expanded)}
                        aria-expanded={expanded}
                        onPress={() => toggleRow(row.id)}
                        className="mt-px flex h-4 w-4 shrink-0 cursor-pointer items-center justify-center rounded-sm border-none bg-transparent p-0 text-muted outline-none hover:text-text focus-visible:ring-2 focus-visible:ring-accent"
                      >
                        {expanded ? <ChevronDownIcon size={12} /> : <ChevronRightIcon size={12} />}
                      </RACButton>
                      <span title={fileLabel} className="block truncate">
                        {fileLabel}
                      </span>
                    </span>
                  </Cell>
                  <Cell className={`${cellClass} capitalize`}>
                    <span className="block truncate">{STAGE_FOR_STEP[row.step] ?? row.step}</span>
                  </Cell>
                  <Cell className={cellClass}>
                    <span
                      title={expanded ? undefined : row.reason}
                      className={
                        expanded
                          ? "block whitespace-pre-wrap break-words"
                          : "line-clamp-2 break-words"
                      }
                    >
                      {row.reason}
                    </span>
                    {expanded && row.items.length > 1 ? (
                      <ul
                        className="mt-2 overflow-y-auto text-muted"
                        style={{ maxHeight: MAX_VISIBLE_FILENAMES * FILENAME_ROW_PX }}
                        // A press on a file name selects it rather than collapsing the row.
                        onPointerDown={(event) => event.stopPropagation()}
                      >
                        {row.items.map((name, fileIndex) => (
                          <li
                            key={`${name}-${String(fileIndex)}`}
                            title={name}
                            className="truncate"
                            style={{ height: FILENAME_ROW_PX }}
                          >
                            {name}
                          </li>
                        ))}
                      </ul>
                    ) : null}
                  </Cell>
                </Row>
              );
            }}
          </TableBody>
        </Table>
      </Virtualizer>
    </div>
  );
}

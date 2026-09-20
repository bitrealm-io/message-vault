import { useVirtualizer } from "@tanstack/react-virtual";
import { type KeyboardEvent, useEffect, useMemo, useRef, useState } from "react";
import { groupImportIssues, type ImportIssueGroup } from "./groupImportIssues";
import type { ImportIssue } from "./ImportSummaryPanel";
import {
  COLLAPSED_ROW_HEIGHT,
  estimateExpandedHeight,
  FILENAME_ROW_PX,
  MAX_VISIBLE_FILENAMES,
  tableViewportHeight,
} from "./importIssuesTableLayout";

const ISSUE_COLUMNS = "grid-cols-[minmax(0,1fr)_4.5rem_minmax(0,1.4fr)]";

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

export default function VirtualizedImportIssuesTable({ issues }: { issues: ImportIssue[] }) {
  const groups = useMemo(() => groupImportIssues(issues), [issues]);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [expandedIndex, setExpandedIndex] = useState<number | null>(null);
  const virtualizer = useVirtualizer({
    count: groups.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (index) =>
      expandedIndex === index
        ? estimateExpandedHeight(groups[index]?.reason ?? "", groups[index]?.items.length ?? 0)
        : COLLAPSED_ROW_HEIGHT,
    overscan: 6,
  });
  const virtualRows = virtualizer.getVirtualItems();
  const expandedGroup = expandedIndex == null ? null : groups[expandedIndex];
  const viewportHeight = tableViewportHeight(
    groups.length,
    expandedGroup == null
      ? null
      : { reason: expandedGroup.reason, fileCount: expandedGroup.items.length },
  );

  useEffect(() => {
    void expandedIndex;
    void groups;
    virtualizer.measure();
  }, [expandedIndex, groups, virtualizer]);

  const toggleRow = (index: number) => {
    setExpandedIndex((current) => (current === index ? null : index));
  };

  const onRowKeyDown = (event: KeyboardEvent<HTMLDivElement>, index: number) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      toggleRow(index);
    }
  };

  return (
    // biome-ignore lint/a11y/useSemanticElements: virtualized grid cannot use native table elements
    <div
      role="table"
      aria-label="Import errors"
      aria-rowcount={groups.length + 1}
      className="mt-2 w-full min-w-0 max-w-full overflow-hidden rounded-lg border border-border text-left text-[0.813rem]"
    >
      {/* biome-ignore lint/a11y/useSemanticElements: virtualized grid cannot use native table elements */}
      <div role="rowgroup" className="border-b border-border bg-elevated">
        {/* biome-ignore lint/a11y/useFocusableInteractive: virtualized grid cannot use native table elements */}
        {/* biome-ignore lint/a11y/useSemanticElements: virtualized grid cannot use native table elements */}
        <div role="row" aria-rowindex={1} className={`grid ${ISSUE_COLUMNS} text-muted`}>
          {/* biome-ignore lint/a11y/useFocusableInteractive: virtualized grid cannot use native table elements */}
          {/* biome-ignore lint/a11y/useSemanticElements: virtualized grid cannot use native table elements */}
          <div role="columnheader" className="min-w-0 px-3 py-2 font-medium">
            Parse File
          </div>
          {/* biome-ignore lint/a11y/useFocusableInteractive: virtualized grid cannot use native table elements */}
          {/* biome-ignore lint/a11y/useSemanticElements: virtualized grid cannot use native table elements */}
          <div role="columnheader" className="px-3 py-2 font-medium">
            Stage
          </div>
          {/* biome-ignore lint/a11y/useFocusableInteractive: virtualized grid cannot use native table elements */}
          {/* biome-ignore lint/a11y/useSemanticElements: virtualized grid cannot use native table elements */}
          <div role="columnheader" className="min-w-0 px-3 py-2 font-medium">
            Error Message
          </div>
        </div>
      </div>
      {/* biome-ignore lint/a11y/useSemanticElements: virtualized grid cannot use native table elements */}
      <div
        ref={scrollRef}
        role="rowgroup"
        className="overflow-x-hidden overflow-y-auto outline-none"
        style={{ height: viewportHeight }}
      >
        <div className="relative w-full min-w-0" style={{ height: virtualizer.getTotalSize() }}>
          {virtualRows.map((virtualRow) => {
            const group = groups[virtualRow.index];
            if (!group) return null;
            const expanded = expandedIndex === virtualRow.index;
            const fileLabel = parseFileLabel(group);
            return (
              // biome-ignore lint/a11y/useSemanticElements: virtualized grid cannot use native table elements
              <div
                key={`${group.kind}-${group.step}-${group.reason}-${virtualRow.index}`}
                data-index={virtualRow.index}
                // Collapsed rows are exactly COLLAPSED_ROW_HEIGHT, so only the
                // expanded row — whose height is a guess — needs measuring.
                // Measuring all of them forced a layout read per visible row on
                // every pass for no gain.
                ref={expanded ? virtualizer.measureElement : undefined}
                role="row"
                tabIndex={0}
                aria-rowindex={virtualRow.index + 2}
                aria-expanded={expanded}
                aria-label={rowAriaLabel(group, expanded)}
                onClick={(event) => {
                  const target = event.target;
                  if (target instanceof Element && target.closest("[data-issue-filenames]")) {
                    return;
                  }
                  toggleRow(virtualRow.index);
                }}
                onKeyDown={(event) => onRowKeyDown(event, virtualRow.index)}
                className={`absolute left-0 top-0 grid w-full min-w-0 cursor-pointer ${ISSUE_COLUMNS} items-start border-b border-border outline-none last:border-b-0 hover:bg-hover focus-visible:bg-hover focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent ${
                  expanded ? "bg-hover" : ""
                }`}
                style={{ transform: `translateY(${virtualRow.start}px)` }}
              >
                {/* biome-ignore lint/a11y/useSemanticElements: virtualized grid cannot use native table elements */}
                <div
                  role="cell"
                  title={fileLabel}
                  className="min-w-0 overflow-hidden px-3 py-2 text-text"
                >
                  <span className="block truncate">{fileLabel}</span>
                </div>
                {/* biome-ignore lint/a11y/useSemanticElements: virtualized grid cannot use native table elements */}
                <div role="cell" className="overflow-hidden px-3 py-2 capitalize text-text">
                  <span className="block truncate">{STAGE_FOR_STEP[group.step] ?? group.step}</span>
                </div>
                {/* biome-ignore lint/a11y/useSemanticElements: virtualized grid cannot use native table elements */}
                <div
                  role="cell"
                  title={expanded ? undefined : group.reason}
                  className="min-w-0 overflow-hidden px-3 py-2 text-text"
                >
                  <span
                    className={
                      expanded
                        ? "block whitespace-pre-wrap break-words"
                        : "line-clamp-2 break-words"
                    }
                  >
                    {group.reason}
                  </span>
                  {expanded && group.items.length > 1 ? (
                    <ul
                      data-issue-filenames=""
                      className="mt-2 overflow-y-auto text-muted"
                      style={{ maxHeight: MAX_VISIBLE_FILENAMES * FILENAME_ROW_PX }}
                    >
                      {group.items.map((name, fileIndex) => (
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
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

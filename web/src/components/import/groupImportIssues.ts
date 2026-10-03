import type { ImportIssue, ImportIssueStage } from "./ImportSummaryPanel";

/**
 * The name of each Stage (CONTEXT.md) an issue can come from. Keyed by every
 * Stage, so it is also the list a stored issue's `stage` is checked against.
 */
export const ISSUE_STAGE_LABEL: Record<ImportIssueStage, string> = {
  staging: "Staging",
  media: "Media",
  upload: "Upload",
};

/** Whether `value` is a Stage an issue can come from. */
export function isIssueStage(value: unknown): value is ImportIssueStage {
  return typeof value === "string" && Object.hasOwn(ISSUE_STAGE_LABEL, value);
}

export type ImportIssueGroup = {
  kind: string;
  stage: ImportIssueStage;
  reason: string;
  items: string[];
};

export function groupImportIssues(issues: ImportIssue[]): ImportIssueGroup[] {
  const groups: ImportIssueGroup[] = [];
  const indexByKey = new Map<string, number>();

  for (const issue of issues) {
    const key = `${issue.kind}\0${issue.stage}\0${issue.reason}`;
    const existing = indexByKey.get(key);
    if (existing == null) {
      indexByKey.set(key, groups.length);
      groups.push({
        kind: issue.kind,
        stage: issue.stage,
        reason: issue.reason,
        items: [issue.item],
      });
      continue;
    }
    const group = groups[existing];
    if (group) {
      group.items.push(issue.item);
    }
  }

  return groups;
}

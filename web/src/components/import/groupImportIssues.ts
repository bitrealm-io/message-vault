import type { ImportIssue } from "./ImportSummaryPanel";
import type { ImportIssueStage } from "./importIssueStage";

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

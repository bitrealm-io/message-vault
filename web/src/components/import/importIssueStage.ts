import type { components } from "../../lib/serverApi.types";

/** The Stage (CONTEXT.md) an issue came from, as the server records it. */
export type ImportIssueStage = components["schemas"]["ImportIssueStage"];

/**
 * The name of each Stage an issue can come from. Keyed by every Stage, so it
 * is also the list a stored issue's `stage` is checked against.
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

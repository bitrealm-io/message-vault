import type { SizeVerdict } from "../../lib/tauri";
import type { AttachmentMediaMode } from "../../lib/types";
import type { GateDelta } from "./gateDelta";
import { pluralFiles, verdictCopy } from "./gateForecast";

/** One sentence about how Media's result differed from the plan. */
export interface DeltaRow {
  key: string;
  text: string;
}

/**
 * The delta rows worth showing, worst first. A bucket with nothing in it
 * is dropped; a row reading "0 files" is noise. Does not include
 * still-pending rows, which render unconditionally alongside this, not as
 * part of it (see `stillPendingRows`).
 */
export function deltaRows(delta: GateDelta): DeltaRow[] {
  const regressed = delta.stillFlagged.filter((item) => item.regressed);
  return [
    {
      key: "lost",
      text:
        // This count cannot say why (crossing the size limit and a
        // conversion failure land here the same way), so the copy states
        // the effect, not a cause the data cannot support.
        delta.lostCount > 0 ? `${pluralFiles(delta.lostCount)} will not be uploaded.` : "",
    },
    {
      key: "regressed",
      text:
        regressed.length > 0
          ? // These were under the limit (or not flagged at all) at the last
            // check, and are now over. They were processed; this is not
            // "could not be processed".
            `${pluralFiles(regressed.length)} that were fine at the last check are now over the limit.`
          : "",
    },
    {
      key: "cameOutFine",
      text:
        delta.cameOutFine > 0
          ? `${pluralFiles(delta.cameOutFine)} written off as too big came in under the limit after all.`
          : "",
    },
  ].filter((row) => row.text.length > 0);
}

/**
 * The not-yet-resolved rows (still flagged, not regressed, e.g. a
 * `cannot_process` file every mode leaves alone) grouped by verdict, in the
 * Staging Approval's own wording. Rendered unconditionally, never folded
 * into `hasChanges`: an import holding nothing but an unconvertible file
 * has no "delta" to report, but "will not upload" is still true and must
 * not be hidden behind "everything came out as expected".
 */
export function stillPendingRows(delta: GateDelta, mode: AttachmentMediaMode): DeltaRow[] {
  const counts = new Map<SizeVerdict, number>();
  for (const item of delta.stillFlagged) {
    if (item.regressed) continue;
    counts.set(item.verdict, (counts.get(item.verdict) ?? 0) + 1);
  }
  return [...counts.entries()].map(([verdict, count]) => {
    const copy = verdictCopy(verdict, mode);
    return {
      key: `pending-${verdict}`,
      text: `${pluralFiles(count)} — ${copy.label}`,
    };
  });
}

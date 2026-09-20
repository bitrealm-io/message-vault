import type { AttachmentForecast, StagingSummary } from "../../lib/tauri";
import type { AttachmentMediaMode } from "../../lib/types";

/** The job the Media stage is doing, in the person's words. */
export function mediaJobVerb(mode: AttachmentMediaMode): "converting" | "compressing" | null {
  if (mode === "convert") return "converting";
  if (mode === "compress") return "compressing";
  return null;
}

/** Largest first: the file most likely to matter leads its list. */
function bySizeDescending(a: AttachmentForecast, b: AttachmentForecast): number {
  return b.sizeBytes - a.sizeBytes;
}

/**
 * The staged files larger than the limit right now. Exact, read from the
 * sizes on disk: no estimate is involved, so this is the same list before
 * and after Media, measured against whatever the folder holds at the time.
 */
export function filesOverLimit(summary: StagingSummary): AttachmentForecast[] {
  return summary.forecasts
    .filter((row) => row.sizeBytes > summary.assetMaxBytes)
    .sort(bySizeDescending);
}

export type EstimatePileKey = "likely_within" | "may_exceed" | "not_media";

/** One group of files the Staging Approval's estimates sort into. */
export interface EstimatePile {
  key: EstimatePileKey;
  label: string;
  /** What the pile means, shown above its files once it is opened. */
  note: string;
  files: AttachmentForecast[];
  /** False for files Media leaves alone: they have no second size to show. */
  showsEstimate: boolean;
}

/**
 * The Staging Approval's estimates, before Media has run: three piles, the
 * empty ones dropped. The backend's five verdicts fold into these. A file
 * under the limit that Media may push over it (`may_grow`) sits with the
 * files expected to stay over it, because both may be left out of the vault
 * and that is what the person is weighing. `fits_as_is` files carry no
 * forecast row at all.
 */
export function estimatePiles(summary: StagingSummary): EstimatePile[] {
  const of = (...verdicts: AttachmentForecast["verdict"][]) =>
    summary.forecasts.filter((row) => verdicts.includes(row.verdict)).sort(bySizeDescending);
  const piles: EstimatePile[] = [
    {
      key: "likely_within",
      label: "Likely within limit",
      note: "Over the limit as staged, and expected to come out under it.",
      files: of("likely_fits"),
      showsEstimate: true,
    },
    {
      key: "may_exceed",
      label: "May exceed limit",
      note: "Expected to come out over the limit. That includes files under it now that are expected to come out larger.",
      files: of("probably_too_big", "may_grow"),
      showsEstimate: true,
    },
    {
      key: "not_media",
      label: "Not audio or video",
      note: "Over the limit, and not a kind of file Media changes.",
      files: of("cannot_process"),
      showsEstimate: false,
    },
  ];
  return piles.filter((pile) => pile.files.length > 0);
}

/** "Compression estimates" or "Conversion estimates"; null when there is no Media stage. */
export function estimatesHeading(mode: AttachmentMediaMode): string | null {
  if (mode === "compress") return "Compression estimates";
  if (mode === "convert") return "Conversion estimates";
  return null;
}

import type { PathStat } from "./tauri";
import { discardImport, listImports, setImportStage as setStage } from "./vaultApi";

/** Where a live import session is. Mirrors the vault's `ImportStage`. */
export const IMPORT_STAGES = [
  "parse",
  "write",
  "awaiting_gate_1",
  "transcode",
  "awaiting_gate_2",
  "pushing",
] as const;

export type ImportStage = (typeof IMPORT_STAGES)[number];

/** The vault sends `stage` as a plain string; anything unknown reads as null. */
function asImportStage(value: string | null | undefined): ImportStage | null {
  return IMPORT_STAGES.includes(value as ImportStage) ? (value as ImportStage) : null;
}

/** Identity of the backup a session was started from. */
export type SourceFingerprint = {
  path: string;
  size_bytes: number;
  modified_unix_ms: number | null;
  /** Filled in after parse; null until then. */
  message_count: number | null;
};

/** The account's live import session, as the vault reports it. */
export type ActiveImportSession = {
  id: number;
  source: string;
  mode: string;
  status: string;
  started_at: string;
  stage: ImportStage | null;
  staging_dir: string | null;
  device_id: string | null;
  form: unknown;
  source_fingerprint: SourceFingerprint | null;
  /** Addresses the backup's device sent from (JSON array), or null. */
  source_identities: unknown;
  /** What was approved at the last gate passed, or null. Mirrors what
   * `setImportStage`'s `approvedPlan` argument last wrote. */
  summary: unknown;
};

/**
 * The account's running Import Run, or null when there is none. At most one
 * runs at a time, so the first item of `status=running` is the one.
 */
export async function getActiveImportSession(): Promise<ActiveImportSession | null> {
  const session = (await listImports({ status: "running", limit: 1 })).items[0];
  if (!session) return null;
  return {
    ...session,
    stage: asImportStage(session.stage),
    staging_dir: session.staging_dir ?? null,
    device_id: session.device_id ?? null,
    source_fingerprint: session.source_fingerprint as SourceFingerprint | null,
  };
}

/**
 * Move a live session to another stage.
 *
 * `approvedPlan`, when given, is recorded as the session's `summary_json` —
 * what the user approved at the gate they just passed. Omitting it leaves
 * whatever plan is already stored untouched; it is never nulled out.
 */
export async function setImportStage(
  id: number,
  stage: ImportStage,
  approvedPlan?: unknown,
): Promise<void> {
  await setStage(id, { stage, summary: approvedPlan });
}

/** Close a session the user gave up on, freeing the account's slot. */
export async function discardImportSession(id: number): Promise<void> {
  await discardImport(id);
}

/**
 * Identity of the backup this session reads.
 *
 * The message count is unknown until parse finishes, so it starts null
 * and is filled in afterwards.
 *
 * The size and mtime come from a stat of the path itself, so for a
 * directory source -- an iOS backup folder, a WhatsApp folder -- they
 * describe the directory entry rather than its contents, and neither moves
 * when a file inside it grows. Nothing reads this fingerprint back yet;
 * whatever does will need its own answer for directories.
 */
export function buildSourceFingerprint(path: string, stat: PathStat): SourceFingerprint {
  return {
    path,
    size_bytes: stat.sizeBytes,
    modified_unix_ms: stat.modifiedUnixMs,
    message_count: null,
  };
}

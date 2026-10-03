/**
 * Browser storage keys for Settings → System in the desktop app.
 *
 * The Staging Directory is not here: the desktop process keeps it
 * (`invokeStagingRoot`, `invokeSetStagingRoot`), because it alone decides
 * which staging folders its commands act on.
 */

import { readPref, removePref, writePref } from "./storage";

const REMEMBER_IMPORTER_PATHS_KEY = "mc-remember-importer-paths";
const IMPORTER_PATHS_KEY = "mc-importer-paths";
const IMPORTER_EXTRA_PATHS_KEY = "mc-importer-extra-paths";

/**
 * Strip trailing `/` or `\\` without turning a Unix root into an empty string.
 */
export function stripTrailingPathSeparators(path: string): string {
  const trimmed = path.trim();
  const stripped = trimmed.replace(/[/\\]+$/, "");
  if (!stripped && /^[/\\]+$/.test(trimmed)) return "/";
  return stripped;
}

/** True when Import should reuse the last backup folder for each source. */
export function getRememberImporterPaths(): boolean {
  return readPref(REMEMBER_IMPORTER_PATHS_KEY) === "1";
}

export function setRememberImporterPaths(on: boolean): void {
  if (on) writePref(REMEMBER_IMPORTER_PATHS_KEY, "1");
  else removePref(REMEMBER_IMPORTER_PATHS_KEY);
}

function readImporterPaths(): Record<string, string> {
  const raw = readPref(IMPORTER_PATHS_KEY);
  if (!raw) return {};
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return {};
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries(parsed)) {
      if (typeof v === "string" && v.trim()) out[k] = v.trim();
    }
    return out;
  } catch {
    return {};
  }
}

function writeImporterPaths(map: Record<string, string>): void {
  if (Object.keys(map).length === 0) removePref(IMPORTER_PATHS_KEY);
  else writePref(IMPORTER_PATHS_KEY, JSON.stringify(map));
}

/** Last backup folder remembered for this import source. */
export function getImporterPath(sourceId: string): string {
  return readImporterPaths()[sourceId] ?? "";
}

export function setImporterPath(sourceId: string, path: string): void {
  const map = readImporterPaths();
  const trimmed = path.trim();
  if (trimmed) {
    writeImporterPaths({ ...map, [sourceId]: trimmed });
    return;
  }
  const next: Record<string, string> = {};
  for (const [key, value] of Object.entries(map)) {
    if (key !== sourceId) next[key] = value;
  }
  writeImporterPaths(next);
}

type ImporterExtraRow = {
  attachmentRoot?: string;
  appleContacts?: string;
  whatsappWa?: string;
  whatsappMedia?: string;
  whatsappDb?: string;
};

function readImporterExtraPaths(): Record<string, ImporterExtraRow> {
  const raw = readPref(IMPORTER_EXTRA_PATHS_KEY);
  if (!raw) return {};
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return {};
    const out: Record<string, ImporterExtraRow> = {};
    for (const [sourceId, row] of Object.entries(parsed)) {
      if (!row || typeof row !== "object") continue;
      const entry: ImporterExtraRow = {};
      const record = row as Record<string, unknown>;
      if (typeof record.attachmentRoot === "string" && record.attachmentRoot.trim()) {
        entry.attachmentRoot = record.attachmentRoot.trim();
      }
      if (typeof record.appleContacts === "string" && record.appleContacts.trim()) {
        entry.appleContacts = record.appleContacts.trim();
      }
      if (typeof record.whatsappWa === "string" && record.whatsappWa.trim()) {
        entry.whatsappWa = record.whatsappWa.trim();
      }
      if (typeof record.whatsappMedia === "string" && record.whatsappMedia.trim()) {
        entry.whatsappMedia = record.whatsappMedia.trim();
      }
      if (typeof record.whatsappDb === "string" && record.whatsappDb.trim()) {
        entry.whatsappDb = record.whatsappDb.trim();
      }
      if (Object.keys(entry).length > 0) out[sourceId] = entry;
    }
    return out;
  } catch {
    return {};
  }
}

function writeImporterExtraPaths(map: Record<string, ImporterExtraRow>): void {
  if (Object.keys(map).length === 0) removePref(IMPORTER_EXTRA_PATHS_KEY);
  else writePref(IMPORTER_EXTRA_PATHS_KEY, JSON.stringify(map));
}

export type ImporterExtraField =
  | "attachmentRoot"
  | "appleContacts"
  | "whatsappWa"
  | "whatsappMedia"
  | "whatsappDb";

const IMPORTER_EXTRA_FIELDS: ImporterExtraField[] = [
  "attachmentRoot",
  "appleContacts",
  "whatsappWa",
  "whatsappMedia",
  "whatsappDb",
];

export function getImporterExtraPaths(sourceId: string): {
  attachmentRoot: string;
  appleContacts: string;
  whatsappWa: string;
  whatsappMedia: string;
  whatsappDb: string;
} {
  const row = readImporterExtraPaths()[sourceId];
  return {
    attachmentRoot: row?.attachmentRoot ?? "",
    appleContacts: row?.appleContacts ?? "",
    whatsappWa: row?.whatsappWa ?? "",
    whatsappMedia: row?.whatsappMedia ?? "",
    whatsappDb: row?.whatsappDb ?? "",
  };
}

const EMPTY_REMEMBERED_PATHS = {
  backupPath: "",
  attachmentRoot: "",
  appleContacts: "",
  whatsappWa: "",
  whatsappMedia: "",
  whatsappDb: "",
};

/** Last paths to show after a source change. Empty when remembering is off. */
export function loadRememberedImportPaths(sourceId: string): {
  backupPath: string;
  attachmentRoot: string;
  appleContacts: string;
  whatsappWa: string;
  whatsappMedia: string;
  whatsappDb: string;
} {
  if (!getRememberImporterPaths()) {
    return { ...EMPTY_REMEMBERED_PATHS };
  }
  const extras = getImporterExtraPaths(sourceId);
  return {
    backupPath: getImporterPath(sourceId),
    attachmentRoot: extras.attachmentRoot,
    appleContacts: extras.appleContacts,
    whatsappWa: extras.whatsappWa,
    whatsappMedia: extras.whatsappMedia,
    whatsappDb: extras.whatsappDb,
  };
}

export function setImporterExtraPath(
  sourceId: string,
  field: ImporterExtraField,
  path: string,
): void {
  const map = readImporterExtraPaths();
  const trimmed = path.trim();
  if (trimmed) {
    const row = map[sourceId] ?? {};
    writeImporterExtraPaths({ ...map, [sourceId]: { ...row, [field]: trimmed } });
    return;
  }
  const row = map[sourceId];
  if (!row) return;
  const nextRow: ImporterExtraRow = {};
  for (const extraField of IMPORTER_EXTRA_FIELDS) {
    if (extraField === field) continue;
    const value = row[extraField];
    if (value) nextRow[extraField] = value;
  }
  const next: Record<string, ImporterExtraRow> = {};
  for (const [key, value] of Object.entries(map)) {
    if (key === sourceId) {
      if (Object.keys(nextRow).length > 0) next[key] = nextRow;
    } else {
      next[key] = value;
    }
  }
  writeImporterExtraPaths(next);
}

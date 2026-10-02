/** Browser storage keys for Settings → System in the desktop app. */

import { readPref, removePref, writePref } from "./storage";
import { invokeHomeDir } from "./tauri";
import { isTauri } from "./tauri-check";

/** localStorage key for the staging parent folder. */
const STAGING_DIR_KEY = "mc-staging-dir";
const REMEMBER_IMPORTER_PATHS_KEY = "mc-remember-importer-paths";
const IMPORTER_PATHS_KEY = "mc-importer-paths";
const IMPORTER_EXTRA_PATHS_KEY = "mc-importer-extra-paths";

let cachedHomeDir: string | null = null;
let homeDirPromise: Promise<string> | null = null;

/** Default folder name under the user home directory for staging. */
const STAGING_PARENT_NAME = "message-crate";

/**
 * Strip trailing `/` or `\\` without turning a Unix root into an empty string.
 */
export function stripTrailingPathSeparators(path: string): string {
  const trimmed = path.trim();
  const stripped = trimmed.replace(/[/\\]+$/, "");
  if (!stripped && /^[/\\]+$/.test(trimmed)) return "/";
  return stripped;
}

/**
 * True for an absolute folder that is not the filesystem root.
 * Relative paths and `/` would write or open next to the process cwd, or anywhere on disk.
 */
export function isUsableStagingParent(path: string): boolean {
  const parent = stripTrailingPathSeparators(path);
  if (!parent || parent === "/") return false;
  if (/^[A-Za-z]:$/.test(parent)) return false;
  if (parent.startsWith("/")) return true;
  if (/^[A-Za-z]:[\\/]/.test(path.trim())) return true;
  if (parent.startsWith("\\\\")) return true;
  return false;
}

/**
 * Default staging parent: `{home}/message-crate`.
 * When home is empty, returns the relative folder name `message-crate`.
 */
export function defaultStagingDir(homeDir: string): string {
  const home = stripTrailingPathSeparators(homeDir);
  if (!home) return STAGING_PARENT_NAME;
  if (home === "/") return `/${STAGING_PARENT_NAME}`;
  return `${home}/${STAGING_PARENT_NAME}`;
}

/** Folder chosen in Settings as the staging parent. Empty when unset. */
export function getStagingDir(): string {
  return readPref(STAGING_DIR_KEY)?.trim() || "";
}

export function setStagingDir(dir: string): void {
  const trimmed = dir.trim();
  if (trimmed) writePref(STAGING_DIR_KEY, trimmed);
  else removePref(STAGING_DIR_KEY);
}

/**
 * Resolved parent folder for staging (saved override or default).
 * Empty when neither a saved path nor a home directory is available.
 */
export async function resolveStagingParent(): Promise<string> {
  const saved = getStagingDir();
  if (isUsableStagingParent(saved)) {
    return stripTrailingPathSeparators(saved);
  }
  const home = (await getHomeDir()).trim();
  if (!home) return "";
  const fallback = defaultStagingDir(home);
  return isUsableStagingParent(fallback) ? stripTrailingPathSeparators(fallback) : "";
}

/** User home folder from the desktop app. Empty in the browser or when lookup fails. */
export async function getHomeDir(): Promise<string> {
  if (cachedHomeDir != null) return cachedHomeDir;
  if (!isTauri()) {
    cachedHomeDir = "";
    return cachedHomeDir;
  }
  if (!homeDirPromise) {
    homeDirPromise = invokeHomeDir()
      .then((info) => {
        cachedHomeDir = info.path.trim();
        return cachedHomeDir;
      })
      .catch(() => {
        cachedHomeDir = "";
        return cachedHomeDir;
      });
  }
  return homeDirPromise;
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

/** Label used in the staging folder name for an export. */
const EXPORT_STAGING_LABEL = "export";

/**
 * Short name used in staging folder names.
 * Import passes a source id; Export passes `EXPORT_STAGING_LABEL`.
 */
function importerSlugForSource(sourceId: string): string {
  if (sourceId === "imessage-ios") return "iphone-ios";
  if (sourceId === "imessage-macos") return "macos";
  if (sourceId === "imessage-jailbreak") return "iphone-jailbreak";
  return sourceId;
}

/** Local date and time as `YYMMDD-HHMMSS`. */
function formatStagingTimestamp(now: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  const yy = pad(now.getFullYear() % 100);
  const mm = pad(now.getMonth() + 1);
  const dd = pad(now.getDate());
  const hh = pad(now.getHours());
  const mi = pad(now.getMinutes());
  const ss = pad(now.getSeconds());
  return `${yy}${mm}${dd}-${hh}${mi}${ss}`;
}

/** Staging folder name: `staging-<label>-YYMMDD-HHMMSS`. */
function stagingDirName(sourceId: string, now: Date = new Date()): string {
  return `staging-${importerSlugForSource(sourceId)}-${formatStagingTimestamp(now)}`;
}

/**
 * Join a staging parent folder with `staging-<importer>-YYMMDD-HHMMSS`.
 * When the parent is empty, the path is only the staging folder name.
 */
export function joinStagingPath(
  parentDir: string,
  sourceId: string,
  now: Date = new Date(),
): string {
  const name = stagingDirName(sourceId, now);
  const parent = stripTrailingPathSeparators(parentDir);
  if (!parent) return name;
  if (parent === "/") return `/${name}`;
  return `${parent}/${name}`;
}

/**
 * Full path for a new export staging folder under the Settings parent.
 *
 * Export stages here only when the chosen format is not JSONL: `message-crate-pull`
 * writes JSONL, and `message-reexport` refuses to convert a folder into
 * itself, so the two steps need separate folders. The folder is deleted once
 * the conversion finishes.
 *
 * @throws If neither a saved staging parent nor the user home directory is
 * available, for the same reason as the import staging folder below.
 */
export async function resolveExportStagingDir(now: Date = new Date()): Promise<string> {
  const parent = await resolveStagingParent();
  if (!parent) {
    throw new Error("Could not determine the user home directory. Staging needs ~/message-crate/.");
  }
  return joinStagingPath(parent, EXPORT_STAGING_LABEL, now);
}

/**
 * Full path for a new import staging folder under the Settings parent
 * (default `{home}/message-crate`).
 *
 * @throws If neither a saved staging parent nor the user home directory is
 * available. A relative `message-crate/…` path would otherwise be created next
 * to the process working directory (for example the AppImage mount).
 */
export async function resolveImportStagingDir(
  _backupPath: string,
  sourceId: string,
): Promise<string> {
  const parent = await resolveStagingParent();
  if (!parent) {
    throw new Error("Could not determine the user home directory. Staging needs ~/message-crate/.");
  }
  return joinStagingPath(parent, sourceId);
}

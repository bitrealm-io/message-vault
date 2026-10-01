/** Backup import and export stay in the desktop app. */
export function canUseImportExport(isTauriApp: boolean): boolean {
  return isTauriApp;
}

/**
 * Same rule as `canUseImportExport`, but a missing profile is not allowed.
 * Loading and a failed profile request both pass null here, so Import/Export stay hidden.
 */
export function canUseImportExportWithProfile(
  isTauriApp: boolean,
  profile: unknown | null | undefined,
): boolean {
  if (profile == null) {
    return false;
  }
  return canUseImportExport(isTauriApp);
}

/**
 * Convert (Settings → Convert) rewrites a folder of exported files and never
 * reads a backup or the server, so it needs the desktop app and nothing else:
 * no profile, no import or export permission.
 */
export function canUseConvert(isTauriApp: boolean): boolean {
  return isTauriApp;
}

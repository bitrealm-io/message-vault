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

/** The two desktop-only screens an account needs a permission for. */
export type ImportExportFeature = "import" | "export";

/**
 * Why an account may not use Import or Export, or null when it may.
 *
 * The left panel shows both entries to every account in the desktop app. A
 * hidden entry says nothing about why, so the screen stays and explains.
 * `demo` is the Demo Account on Import: the way forward there is an account of
 * the person's own, not a word with the Owner.
 */
export type ImportExportBlock = "demo" | "not-allowed";

export function importExportBlock(
  feature: ImportExportFeature,
  profile: { can_import: boolean; can_export: boolean; is_demo: boolean },
): ImportExportBlock | null {
  if (feature === "export") {
    return profile.can_export ? null : "not-allowed";
  }
  // The server refuses every import into the Demo Account by its id, whatever
  // its permission row says (`docs/adr/0016-the-demo-account-is-fixed-not-configured.md`).
  if (profile.is_demo) {
    return "demo";
  }
  return profile.can_import ? null : "not-allowed";
}

/**
 * Convert (Settings → Convert) rewrites a folder of exported files and never
 * reads a backup or the server, so it needs the desktop app and nothing else:
 * no profile, no import or export permission.
 */
export function canUseConvert(isTauriApp: boolean): boolean {
  return isTauriApp;
}

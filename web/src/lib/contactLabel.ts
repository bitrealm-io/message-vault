/**
 * The text a contact goes by on screen: its preferred name, or, when it has
 * none, its first identity. Empty only for a contact with neither.
 */
export function contactLabelText(name: string, handles: readonly string[] | undefined): string {
  return name.trim() || (handles ?? []).find((h) => h.trim())?.trim() || "";
}

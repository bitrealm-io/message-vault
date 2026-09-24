/**
 * The text a contact goes by on screen: its preferred name, or, when it has
 * none, its first identity. Empty only for a contact with neither.
 */
export function contactLabelText(name: string, addresses: readonly string[] | undefined): string {
  return name.trim() || (addresses ?? []).find((h) => h.trim())?.trim() || "";
}

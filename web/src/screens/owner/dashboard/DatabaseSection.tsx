import { formatBytes } from "../../settings/storage/storageUtils";
import { DashboardSection } from "./DashboardSection";
import type { VaultStorage } from "./types";

/** One measured figure with its name under it. */
function Figure({ label, bytes }: { label: string; bytes: number }) {
  return (
    <div className="min-w-[8rem] flex-1">
      <div className="text-[1.375rem] font-semibold text-text">{formatBytes(bytes)}</div>
      <div className="mt-1 text-[0.813rem] text-muted">{label}</div>
    </div>
  );
}

/**
 * The database on disk: its size, how much of it the messages take, and how
 * much the full-text search index adds. All three are measured by the vault,
 * on either engine. The search figure is for the whole vault, because the
 * index is one shared structure and cannot be split by account.
 */
export function DatabaseSection({ storage }: { storage: VaultStorage }) {
  return (
    <DashboardSection
      title="Database"
      hint="The database size excludes attachment files, which are counted under Vault contents."
    >
      <div className="flex flex-wrap gap-6 rounded-xl border border-border bg-elevated p-4">
        <Figure label="Database size" bytes={storage.database_bytes} />
        <Figure label="Messages on disk" bytes={storage.messages_bytes} />
        <Figure label="Full-text search index" bytes={storage.fts_bytes} />
      </div>
    </DashboardSection>
  );
}

import { useCallback } from "react";
import { apiErrorMessage } from "../lib/apiErrorMessage";
import { keys } from "../lib/queryKeys";
import { useRouteQuery } from "../lib/routeQuery";
import { getConversationSources } from "../lib/serverApi";
import ModalShell from "./ModalShell";

export default function SourcesPanel({
  conversationId,
  onClose,
}: {
  conversationId: number | null;
  onClose: () => void;
}) {
  const fetchSources = useCallback(
    async (signal: AbortSignal) => {
      if (!conversationId) return [];
      return getConversationSources(conversationId, { signal });
    },
    [conversationId],
  );

  const {
    data,
    isPending: loading,
    error,
  } = useRouteQuery(keys.conversations.sources(conversationId), fetchSources, {
    enabled: conversationId !== null,
  });

  if (!conversationId) return null;

  const sources = data ?? [];
  const total = sources.reduce((sum, s) => sum + s.unique_count, 0);

  return (
    <ModalShell
      open={!!conversationId}
      onOpenChange={(o) => {
        if (o) return;
        onClose();
      }}
      label="Sources"
      title="Sources"
      onClose={onClose}
      variant="drawer"
    >
      {loading ? (
        <div className="text-[0.875rem] text-muted">Loading…</div>
      ) : error ? (
        <div className="rounded border border-danger-soft-border bg-danger-soft-bg p-2 text-[0.813rem] text-danger">
          {apiErrorMessage(error, "Could not load sources.")}
        </div>
      ) : sources.length === 0 ? (
        <div className="text-[0.875rem] text-muted">No source data available.</div>
      ) : (
        <>
          {sources.map((s) => (
            <div key={s.backup_name} className="mb-3 rounded bg-elevated p-2">
              <div className="text-[0.875rem] font-medium">{s.backup_name}</div>
              <div className="text-[0.75rem] text-muted">
                {s.message_count.toLocaleString()} messages
              </div>
              <div className="text-[0.75rem] text-muted">
                {s.unique_count.toLocaleString()} unique ({s.percentage}% of unique messages)
              </div>
            </div>
          ))}
          <div className="mt-2 text-[0.813rem] text-muted">
            Net total: {total.toLocaleString()} unique messages
          </div>
        </>
      )}
    </ModalShell>
  );
}

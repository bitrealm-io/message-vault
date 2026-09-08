import { useCallback, useEffect, useMemo, useState } from "react";
import ConversationRow from "../components/ConversationRow";
import ConversationSortMenu from "../components/ConversationSortMenu";
import ListRangeHeader from "../components/ListRangeHeader";
import ListRangePill, {
  RANGE_PILL_OVERLAY_INSET,
  RANGE_PILL_SCROLL_PAD,
} from "../components/ListRangePill";
import TagsMenu from "../components/TagsMenu";
import { useSetRightToolbar } from "../components/useRightToolbar";
import VirtualList, { type VisibleRange } from "../components/VirtualList";
import { apiErrorMessage } from "../lib/apiErrorMessage";
import {
  type ConversationSortState,
  loadConversationSort,
  saveConversationSort,
} from "../lib/conversationSort";
import { formatVisibleRange } from "../lib/listPaging";
import { checksFromMembers } from "../lib/membershipChecks";
import { useMessageTagActions, useSetMessageTagMembers } from "../lib/messageTags";
import { hasFieldToken } from "../lib/searchFields";
import type { Conversation } from "../lib/types";
import { useMessageTags } from "../lib/useMessageTags";
import { listConversations } from "../lib/vaultApi";
import { keys } from "../lib/vaultKeys";
import { type PagedFetchPage, useVaultPagedList } from "../lib/vaultQuery";

const QUERY_DEBOUNCE_MS = 300;

export default function ConversationList({
  selectedId,
  onSelect,
  query,
}: {
  selectedId: number | null;
  onSelect: (conversation: Conversation) => void;
  query: string;
}) {
  const tagActions = useMessageTagActions();
  const setTagMembers = useSetMessageTagMembers();
  const [debouncedQ, setDebouncedQ] = useState(query);
  const [visibleRange, setVisibleRange] = useState<VisibleRange>({ start: 0, end: 0 });
  const [checkedIds, setCheckedIds] = useState<Set<number>>(() => new Set());
  const [sortState, setSortState] = useState<ConversationSortState>(() => loadConversationSort());
  const { tags: allTags } = useMessageTags();
  const setRightToolbar = useSetRightToolbar();

  useEffect(() => {
    void query;
    setCheckedIds(new Set());
  }, [query]);

  useEffect(() => {
    // A query that names a word applies at once, so the list does not flash empty.
    if (hasFieldToken(query)) {
      setDebouncedQ(query);
      return;
    }
    const t = window.setTimeout(() => setDebouncedQ(query), QUERY_DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [query]);

  const fetchPage = useCallback<PagedFetchPage<Conversation>>(
    async ({ limit, offset, signal }) => {
      const res = await listConversations(
        {
          q: debouncedQ,
          limit,
          offset,
          // ADR-0009: one `sort` parameter, a leading `-` for descending.
          sort: sortState.order === "desc" ? `-${sortState.sort}` : sortState.sort,
        },
        { signal },
      );
      return {
        items: res.items,
        total: res.total,
      };
    },
    [debouncedQ, sortState],
  );

  const {
    items: conversations,
    total,
    loading,
    refreshing,
    filling,
    error,
    hasMore,
    loadMore,
  } = useVaultPagedList(
    keys.conversations.list({ q: debouncedQ, sort: sortState.sort, order: sortState.order }),
    fetchPage,
  );

  const selectedConversation = conversations.find((c) => c.id === selectedId) ?? null;
  const targetConversations = useMemo(() => {
    if (checkedIds.size > 0) {
      return conversations.filter((c) => checkedIds.has(c.id));
    }
    return selectedConversation ? [selectedConversation] : [];
  }, [checkedIds, conversations, selectedConversation]);
  const tagChecks = useMemo(
    () =>
      checksFromMembers(
        allTags,
        targetConversations.map((c) => c.tags ?? []),
      ),
    [allTags, targetConversations],
  );

  const applyMembership = useCallback(
    (name: string, enable: boolean) => {
      const ids = targetConversations.map((c) => c.id);
      if (ids.length === 0) return Promise.resolve();
      // The tags on the rows change in the cache before the vault answers and
      // go back if it refuses, so nothing here has to remember them. Marking
      // every conversation stale afterwards is what used to need the
      // `membershipRev` counter in the query key.
      return setTagMembers
        .mutateAsync({ name, patch: enable ? { add: ids } : { remove: ids } })
        .then(
          () => undefined,
          () => undefined,
        );
    },
    [targetConversations, setTagMembers.mutateAsync],
  );

  useEffect(() => {
    setRightToolbar(
      <TagsMenu
        allTags={allTags}
        checks={tagChecks}
        disabled={targetConversations.length === 0}
        onToggle={(name) => {
          const on = tagChecks[name] === "on";
          void applyMembership(name, !on);
        }}
        onCreate={(name) => {
          void (async () => {
            const existing = allTags.find((t) => t.toLowerCase() === name.toLowerCase());
            if (!existing) {
              await tagActions.create(name);
            }
            await applyMembership(existing ?? name, true);
          })();
        }}
        onClearAll={() => {
          const names = new Set<string>();
          for (const c of targetConversations) {
            for (const t of c.tags ?? []) names.add(t);
          }
          void (async () => {
            for (const name of names) {
              await applyMembership(name, false);
            }
          })();
        }}
      />,
    );
    return () => setRightToolbar(null);
  }, [
    allTags,
    applyMembership,
    setRightToolbar,
    tagChecks,
    targetConversations,
    tagActions.create,
  ]);

  const selectAllChecked =
    conversations.length > 0 && conversations.every((c) => checkedIds.has(c.id));
  const selectAllIndeterminate =
    !selectAllChecked && conversations.some((c) => checkedIds.has(c.id));

  const rangeLabel =
    loading && conversations.length === 0
      ? "Loading…"
      : formatVisibleRange(visibleRange.start, visibleRange.end, total, conversations.length);
  // Once there are rows the count rides at the bottom of the panel, the way the
  // contact list shows it; the header keeps it only while the list is still empty.
  const showRangePill = conversations.length > 0;

  if (error && conversations.length === 0) {
    return (
      <div className="p-4 text-[0.813rem] text-danger">
        {apiErrorMessage(error, "Could not load conversations.")}
      </div>
    );
  }

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <ListRangeHeader
        rangeLabel={showRangePill ? undefined : rangeLabel}
        refreshing={!showRangePill && refreshing}
        filling={!showRangePill && filling}
        selectAllChecked={selectAllChecked}
        selectAllIndeterminate={selectAllIndeterminate}
        onSelectAllChange={(on) => {
          setCheckedIds(on ? new Set(conversations.map((c) => c.id)) : new Set());
        }}
        selectAllLabel="Select all conversations"
        selectAllDisabled={conversations.length === 0}
        actions={
          <ConversationSortMenu
            sort={sortState.sort}
            order={sortState.order}
            onChange={(next) => {
              setSortState(next);
              saveConversationSort(next);
            }}
          />
        }
      />
      <VirtualList
        count={conversations.length}
        estimateSize={64}
        dynamicSize
        onVisibleRangeChange={setVisibleRange}
        visibleBottomInset={RANGE_PILL_OVERLAY_INSET}
        footer={<div aria-hidden className="shrink-0" style={{ height: RANGE_PILL_SCROLL_PAD }} />}
        onNearEnd={() => {
          if (hasMore) loadMore();
        }}
        empty={
          !loading ? <div className="p-4 text-[0.813rem] text-muted">No conversations</div> : null
        }
        renderItem={(index) => {
          const c = conversations[index];
          if (!c) return null;
          return (
            <ConversationRow
              conversation={c}
              isSelected={c.id === selectedId}
              onClick={() => onSelect(c)}
              checked={checkedIds.has(c.id)}
              onCheckChange={(id) => {
                setCheckedIds((prev) => {
                  const next = new Set(prev);
                  if (next.has(id)) next.delete(id);
                  else next.add(id);
                  return next;
                });
              }}
            />
          );
        }}
      />
      {showRangePill ? (
        <ListRangePill
          rangeLabel={rangeLabel}
          refreshing={refreshing}
          filling={filling}
          testId="conversation-list-range-pill"
        />
      ) : null}
    </div>
  );
}

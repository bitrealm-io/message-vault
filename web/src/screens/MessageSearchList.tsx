import { type ReactNode, useCallback, useMemo } from "react";
import ListRangeHeader from "../components/ListRangeHeader";
import MessageSearchRow from "../components/MessageSearchRow";
import SortMenu, { type SortField } from "../components/SortMenu";
import VirtualList from "../components/VirtualList";
import { apiErrorMessage } from "../lib/apiErrorMessage";
import { freeTextTerms } from "../lib/freeTextTerms";
import { MAX_LIST_OFFSET } from "../lib/listPaging";
import { messageCount } from "../lib/messageRowText";
import {
  effectiveMessageSort,
  type MessageSearchSort,
  type MessageSearchSortKey,
  messageSortParam,
} from "../lib/messageSearchSort";
import { keys } from "../lib/queryKeys";
import { type PagedFetchPage, useRoutePagedList } from "../lib/routeQuery";
import { listMessages } from "../lib/serverApi";
import type { Message } from "../lib/types";
import { useDebouncedQuery } from "../lib/useDebouncedQuery";

/** Rows read at a time as the person scrolls. */
const PAGE_SIZE = 40;

const RELEVANCE: SortField<MessageSearchSortKey> = { id: "relevance", label: "Relevance" };
const DATE: SortField<MessageSearchSortKey> = { id: "date", label: "Date" };

/**
 * The Messages list: one row per message the search matches, from every
 * conversation (#313). It reads 40 rows at a time as the person scrolls and
 * shows the total, up to the route's offset ceiling of 50,000. An empty
 * search lists nothing and asks for one.
 *
 * The sort menu offers Relevance only when the query has a free-text word
 * to rank by, and it is then the default; otherwise the default is Date,
 * newest first. A pick is the caller's to keep, so it survives opening a
 * result.
 */
export default function MessageSearchList({
  query,
  sortPick,
  onSortPick,
  selectedId,
  onSelect,
}: {
  query: string;
  sortPick: MessageSearchSort | null;
  onSortPick: (next: MessageSearchSort) => void;
  selectedId: number | null;
  onSelect: (message: Message) => void;
}) {
  const debouncedQ = useDebouncedQuery(query);

  const q = debouncedQ.trim();
  const terms = useMemo(() => freeTextTerms(q), [q]);
  const rankable = terms.length > 0;
  const sort = effectiveMessageSort(sortPick, rankable);

  const sortMenu = (
    <SortMenu
      fields={rankable ? [RELEVANCE, DATE] : [DATE]}
      unordered={["relevance"]}
      sort={sort.sort}
      order={sort.order}
      onChange={onSortPick}
      itemNoun="messages"
      ascLabel="Oldest first"
      descLabel="Newest first"
    />
  );

  if (!q) {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <ListRangeHeader />
        <div className="p-4 text-[0.813rem] text-muted">
          Type a search above to list the messages it matches.
        </div>
      </div>
    );
  }

  return (
    <MessageResults
      key={`${q}\u0000${messageSortParam(sort)}`}
      q={q}
      sort={sort}
      terms={terms}
      sortMenu={sortMenu}
      selectedId={selectedId}
      onSelect={onSelect}
    />
  );
}

function MessageResults({
  q,
  sort,
  terms,
  sortMenu,
  selectedId,
  onSelect,
}: {
  q: string;
  sort: MessageSearchSort;
  terms: ReturnType<typeof freeTextTerms>;
  sortMenu: ReactNode;
  selectedId: number | null;
  onSelect: (message: Message) => void;
}) {
  const sortParam = messageSortParam(sort);
  const fetchPage = useCallback<PagedFetchPage<Message>>(
    async ({ limit, offset, signal }) => {
      const res = await listMessages({ q, sort: sortParam, limit, offset }, { signal });
      return { items: res.items, total: res.total };
    },
    [q, sortParam],
  );
  const { items, total, loading, refreshing, filling, error, hasMore, loadMore } =
    useRoutePagedList(keys.messages.list(q, sortParam), fetchPage, {
      firstPageSize: PAGE_SIZE,
      fillPageSize: PAGE_SIZE,
      maxOffset: MAX_LIST_OFFSET,
    });

  if (error && items.length === 0) {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <ListRangeHeader actions={sortMenu} />
        <div className="p-4 text-[0.813rem] text-danger">
          {apiErrorMessage(error, "Could not load messages.")}
        </div>
      </div>
    );
  }

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <ListRangeHeader
        rangeLabel={loading && items.length === 0 ? "Loading…" : messageCount(total)}
        refreshing={refreshing}
        filling={filling}
        actions={sortMenu}
      />
      <VirtualList
        count={items.length}
        estimateSize={72}
        dynamicSize
        onNearEnd={() => {
          if (hasMore) loadMore();
        }}
        empty={!loading ? <div className="p-4 text-[0.813rem] text-muted">No messages</div> : null}
        renderItem={(index) => {
          const m = items[index];
          if (!m) return null;
          return (
            <MessageSearchRow
              message={m}
              terms={terms}
              isSelected={m.id === selectedId}
              onClick={() => onSelect(m)}
            />
          );
        }}
      />
    </div>
  );
}

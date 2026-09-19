import {
  type CSSProperties,
  type ReactNode,
  type UIEvent,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { ListBox, ListBoxItem, ListLayout, Virtualizer } from "react-aria-components";
import { groupByLetter } from "../lib/contactSort";
import { formatVisibleRange } from "../lib/listPaging";
import { isTauri } from "../lib/tauri-check";
import { listRowDividersThin, resizeHandleGutter } from "../lib/tw";
import ListRangeHeader from "./ListRangeHeader";
import ListRangePill, { RANGE_PILL_OVERLAY_INSET, RANGE_PILL_SCROLL_PAD } from "./ListRangePill";
import VirtualList, { type VisibleRange } from "./VirtualList";

const NEAR_END_THRESHOLD = 10;

type InfiniteOffsetListProps<T> = {
  items: T[];
  total: number;
  loading: boolean;
  refreshing?: boolean;
  filling: boolean;
  error: string;
  hasMore: boolean;
  requestMore: () => void;
  estimateSize: number;
  /** Variable row heights (filter subtitles). Maps to estimatedRowSize on RAC. */
  dynamicSize?: boolean;
  selectedId?: string | null;
  onSelect: (item: T) => void;
  /** When set, drives the highlighted row instead of `selectedId`. */
  isRowHighlighted?: (item: T) => boolean;
  /** Spacer before the A–Z letter so it lines up with the initials column. */
  sectionLead?: ReactNode;
  getId: (item: T) => string;
  /** Accessible name for ListBoxItem (Tauri path). */
  getTextValue?: (item: T) => string;
  renderRow: (item: T) => ReactNode;
  /**
   * Leading cell rendered as a sibling of the row control, not inside it.
   * Row selection is a button, and interactive content (a per-row checkbox)
   * may not nest inside a button — keeping it out is what makes it reachable.
   */
  renderRowLead?: (item: T) => ReactNode;
  empty?: ReactNode;
  ariaLabel: string;
  /** Override the “N of total” denominator (e.g. filtered client count). */
  rangeTotal?: number;
  errorPrefix?: string;
  /** Control on the right of the “N–M of total” row. */
  headerActions?: ReactNode;
  selectAllChecked?: boolean;
  selectAllIndeterminate?: boolean;
  onSelectAllChange?: (checked: boolean) => void;
  selectAllLabel?: string;
  /** Letter for in-list section headers. Omit while searching. */
  getSectionLetter?: (item: T) => string;
};

function rowClass(selected: boolean, hovered = false): string {
  const fill = selected
    ? "bg-hover-strong"
    : hovered
      ? "bg-hover"
      : "bg-transparent hover:bg-hover";
  return `box-border flex w-full cursor-pointer items-center gap-2.5 border-none p-2 px-3 text-left text-text outline-none ${listRowDividersThin} ${fill}`;
}

/**
 * The select-row button when a lead cell sits beside it. The row container owns
 * the fill, dividers and padding, so the button carries no chrome of its own.
 *
 * The button is only as tall as the name inside it, so its `::after` is
 * stretched over the whole row. Without that the row's padding and the gap
 * beside the lead cell show the hover fill and the pointer, then ignore the click.
 */
const ROW_BODY =
  "flex min-w-0 flex-1 cursor-pointer items-center gap-2.5 border-none bg-transparent p-0 text-left text-text outline-none after:absolute after:inset-0 after:content-['']";

/** Lifts the lead cell above the select button's stretched target, so it still takes its own clicks. */
const ROW_LEAD = "relative z-[1] flex shrink-0 self-center";

/** A row is either one button, or a container holding the lead cell plus that button. */
function Row({
  lead,
  className,
  style,
  onSelect,
  children,
  ...rest
}: {
  lead: ReactNode;
  className: string;
  style?: CSSProperties;
  onSelect: () => void;
  children: ReactNode;
  "data-contact-index"?: number;
}) {
  if (!lead) {
    return (
      <button type="button" onClick={onSelect} className={className} style={style} {...rest}>
        {children}
      </button>
    );
  }
  return (
    <div className={`relative ${className}`} style={style} {...rest}>
      <div className={ROW_LEAD}>{lead}</div>
      <button type="button" onClick={onSelect} className={ROW_BODY}>
        {children}
      </button>
    </div>
  );
}

function rangeFromScroll(
  scrollTop: number,
  clientHeight: number,
  estimateSize: number,
  count: number,
  bottomInset = 0,
): VisibleRange {
  if (count === 0 || clientHeight <= 0 || estimateSize <= 0) {
    return { start: 0, end: 0 };
  }
  const visibleHeight = Math.max(0, clientHeight - bottomInset);
  const startIdx = Math.floor(scrollTop / estimateSize);
  const endIdx = Math.min(count - 1, Math.ceil((scrollTop + visibleHeight) / estimateSize) - 1);
  return { start: startIdx + 1, end: Math.max(startIdx, endIdx) + 1 };
}

function RacVirtualList<T extends object>({
  items,
  estimateSize,
  dynamicSize,
  selectedId,
  onSelect,
  isRowHighlighted,
  getId,
  getTextValue,
  renderRow,
  renderRowLead,
  requestMore,
  hasMore,
  onVisibleRangeChange,
  empty,
  ariaLabel,
}: {
  items: T[];
  estimateSize: number;
  dynamicSize: boolean;
  selectedId: string | null;
  onSelect: (item: T) => void;
  isRowHighlighted?: (item: T) => boolean;
  getId: (item: T) => string;
  getTextValue?: (item: T) => string;
  renderRow: (item: T) => ReactNode;
  renderRowLead?: (item: T) => ReactNode;
  requestMore: () => void;
  hasMore: boolean;
  onVisibleRangeChange: (range: VisibleRange) => void;
  empty?: ReactNode;
  ariaLabel: string;
}) {
  const maybeRequestMore = useCallback(
    (end1Based: number) => {
      if (!hasMore || items.length === 0) return;
      if (end1Based >= items.length - NEAR_END_THRESHOLD) {
        requestMore();
      }
    },
    [hasMore, items.length, requestMore],
  );

  const onScroll = (e: UIEvent<HTMLElement>) => {
    const el = e.currentTarget;
    const range = rangeFromScroll(
      el.scrollTop,
      el.clientHeight,
      estimateSize,
      items.length,
      RANGE_PILL_OVERLAY_INSET,
    );
    onVisibleRangeChange(range);
    maybeRequestMore(range.end);
  };

  if (items.length === 0 && empty) {
    return <div className="min-h-0 flex-1 overflow-auto">{empty}</div>;
  }

  const layoutOptions = dynamicSize
    ? { estimatedRowSize: estimateSize }
    : { rowSize: estimateSize };

  return (
    <Virtualizer layout={ListLayout} layoutOptions={layoutOptions}>
      <ListBox
        aria-label={ariaLabel}
        items={items}
        selectionMode="single"
        selectionBehavior="replace"
        selectedKeys={selectedId ? new Set([selectedId]) : new Set()}
        onScroll={onScroll}
        className={`min-h-0 flex-1 overflow-auto outline-none ${resizeHandleGutter}`}
        style={{
          display: "block",
          paddingTop: 0,
          paddingRight: 0,
          paddingBottom: RANGE_PILL_SCROLL_PAD,
          paddingLeft: 0,
        }}
      >
        {(item) => {
          const id = getId(item);
          return (
            <ListBoxItem
              id={id}
              textValue={getTextValue?.(item) ?? id}
              onAction={() => onSelect(item)}
              className={({ isSelected, isHovered }) =>
                rowClass(isRowHighlighted?.(item) ?? isSelected, isHovered)
              }
              style={dynamicSize ? { minHeight: estimateSize } : { height: "100%", minHeight: 0 }}
            >
              {/* A row checkbox inside a listbox option is RAC's own selection pattern. */}
              {renderRowLead?.(item)}
              {renderRow(item)}
            </ListBoxItem>
          );
        }}
      </ListBox>
    </Virtualizer>
  );
}

function TanStackVirtualList<T>({
  items,
  estimateSize,
  dynamicSize,
  selectedId,
  onSelect,
  isRowHighlighted,
  getId,
  renderRow,
  renderRowLead,
  requestMore,
  hasMore,
  onVisibleRangeChange,
  empty,
}: {
  items: T[];
  estimateSize: number;
  dynamicSize: boolean;
  selectedId: string | null;
  onSelect: (item: T) => void;
  isRowHighlighted?: (item: T) => boolean;
  getId: (item: T) => string;
  getTextValue?: (item: T) => string;
  renderRow: (item: T) => ReactNode;
  renderRowLead?: (item: T) => ReactNode;
  requestMore: () => void;
  hasMore: boolean;
  onVisibleRangeChange: (range: VisibleRange) => void;
  empty?: ReactNode;
}) {
  return (
    <VirtualList
      count={items.length}
      estimateSize={estimateSize}
      dynamicSize={dynamicSize}
      nearEndThreshold={NEAR_END_THRESHOLD}
      onVisibleRangeChange={onVisibleRangeChange}
      onNearEnd={() => {
        if (hasMore) requestMore();
      }}
      empty={empty}
      footer={<div aria-hidden className="shrink-0" style={{ height: RANGE_PILL_SCROLL_PAD }} />}
      visibleBottomInset={RANGE_PILL_OVERLAY_INSET}
      renderItem={(index) => {
        const item = items[index];
        if (!item) return null;
        const id = getId(item);
        const selected = isRowHighlighted?.(item) ?? id === selectedId;
        return (
          <Row
            lead={renderRowLead?.(item)}
            onSelect={() => onSelect(item)}
            style={{
              height: dynamicSize ? "auto" : "100%",
              minHeight: dynamicSize ? estimateSize : undefined,
            }}
            className={rowClass(selected)}
          >
            {renderRow(item)}
          </Row>
        );
      }}
    />
  );
}

const LETTER_DIVIDER = "flex items-center border-b border-border bg-panel px-3 py-1";

function SectionedLetterList<T>({
  items,
  selectedId,
  onSelect,
  isRowHighlighted,
  sectionLead,
  getId,
  renderRow,
  renderRowLead,
  requestMore,
  hasMore,
  getSectionLetter,
  currentLetter,
  onVisibleRangeChange,
  empty,
}: {
  items: T[];
  selectedId: string | null;
  onSelect: (item: T) => void;
  isRowHighlighted?: (item: T) => boolean;
  sectionLead?: ReactNode;
  getId: (item: T) => string;
  renderRow: (item: T) => ReactNode;
  renderRowLead?: (item: T) => ReactNode;
  requestMore: () => void;
  hasMore: boolean;
  getSectionLetter: (item: T) => string;
  currentLetter: string | null;
  onVisibleRangeChange: (range: VisibleRange) => void;
  empty?: ReactNode;
}) {
  // This list is not virtualized, so both of these walk every contact. Rebuilding
  // them on each render is what made scrolling a large catalog expensive.
  const groups = useMemo(() => groupByLetter(items, getSectionLetter), [items, getSectionLetter]);
  const indexById = useMemo(
    () => new Map(items.map((item, i) => [getId(item), i])),
    [items, getId],
  );
  const scrollerRef = useRef<HTMLDivElement>(null);
  const onRangeRef = useRef(onVisibleRangeChange);
  onRangeRef.current = onVisibleRangeChange;
  const requestMoreRef = useRef(requestMore);
  requestMoreRef.current = requestMore;
  const hasMoreRef = useRef(hasMore);
  hasMoreRef.current = hasMore;

  const itemCount = items.length;
  const publishVisibleRange = useCallback(
    (root: HTMLElement) => {
      const rootRect = root.getBoundingClientRect();
      const viewTop = rootRect.top;
      const viewBottom = rootRect.bottom - RANGE_PILL_OVERLAY_INSET;
      const rows = root.querySelectorAll<HTMLElement>("[data-contact-index]");

      // Rows run top to bottom, so the first one reaching the viewport can be
      // bisected for. Measuring all of them here cost one layout read per
      // contact on every scroll event — 20k of them on a large catalog.
      let lo = 0;
      let hi = rows.length - 1;
      let first = rows.length;
      while (lo <= hi) {
        const mid = (lo + hi) >> 1;
        if (rows[mid].getBoundingClientRect().bottom > viewTop) {
          first = mid;
          hi = mid - 1;
        } else {
          lo = mid + 1;
        }
      }

      let start = 0;
      let end = 0;
      for (let i = first; i < rows.length; i++) {
        const row = rows[i];
        if (row.getBoundingClientRect().top >= viewBottom) break;
        const raw = row.getAttribute("data-contact-index");
        const idx = raw == null ? Number.NaN : Number(raw);
        if (!Number.isFinite(idx)) continue;
        const oneBased = idx + 1;
        if (start === 0) start = oneBased;
        end = oneBased;
      }
      onRangeRef.current({ start, end });
      if (hasMoreRef.current && itemCount > 0 && end >= itemCount - NEAR_END_THRESHOLD) {
        requestMoreRef.current();
      }
    },
    [itemCount],
  );

  useLayoutEffect(() => {
    const root = scrollerRef.current;
    if (root) publishVisibleRange(root);
  }, [publishVisibleRange]);

  // Scroll events outpace frames; one measurement per frame is all that can show.
  const rangeFrameRef = useRef(0);
  useEffect(
    () => () => {
      if (rangeFrameRef.current) cancelAnimationFrame(rangeFrameRef.current);
    },
    [],
  );

  const onScroll = (e: UIEvent<HTMLDivElement>) => {
    if (rangeFrameRef.current) return;
    const root = e.currentTarget;
    rangeFrameRef.current = requestAnimationFrame(() => {
      rangeFrameRef.current = 0;
      publishVisibleRange(root);
    });
  };

  if (items.length === 0 && empty) {
    return <div className="min-h-0 flex-1 overflow-auto">{empty}</div>;
  }

  return (
    <div
      ref={scrollerRef}
      className={`min-h-0 flex-1 overflow-auto ${resizeHandleGutter}`}
      onScroll={onScroll}
    >
      {groups.map(([letter, groupItems]) => (
        <section key={letter} aria-label={`Names starting with ${letter}`}>
          {letter !== currentLetter ? (
            <div className={`${LETTER_DIVIDER} gap-2.5`}>
              {sectionLead}
              <span className="flex h-7 w-7 shrink-0 items-center justify-center text-[0.75rem] font-semibold text-muted">
                {letter}
              </span>
            </div>
          ) : null}
          {groupItems.map((item) => {
            const id = getId(item);
            const selected = isRowHighlighted?.(item) ?? id === selectedId;
            const index = indexById.get(id) ?? 0;
            return (
              <Row
                key={id}
                lead={renderRowLead?.(item)}
                data-contact-index={index}
                onSelect={() => onSelect(item)}
                className={rowClass(selected)}
              >
                {renderRow(item)}
              </Row>
            );
          })}
        </section>
      ))}
      <div aria-hidden className="shrink-0" style={{ height: RANGE_PILL_SCROLL_PAD }} />
    </div>
  );
}

export default function InfiniteOffsetList<T extends object>({
  items,
  total,
  loading,
  refreshing = false,
  filling,
  error,
  hasMore,
  requestMore,
  estimateSize,
  dynamicSize = false,
  selectedId = null,
  onSelect,
  isRowHighlighted,
  sectionLead,
  getId,
  getTextValue,
  renderRow,
  renderRowLead,
  empty,
  ariaLabel,
  rangeTotal,
  errorPrefix = "Could not load list",
  headerActions,
  selectAllChecked = false,
  selectAllIndeterminate = false,
  onSelectAllChange,
  selectAllLabel,
  getSectionLetter,
}: InfiniteOffsetListProps<T>) {
  const [visibleRange, setVisibleRange] = useState<VisibleRange>({
    start: 0,
    end: 0,
  });
  const denom = rangeTotal ?? total;

  const rangeLabel =
    loading && items.length === 0
      ? "Loading…"
      : formatVisibleRange(visibleRange.start, visibleRange.end, denom, items.length);

  const firstVisibleIndex = visibleRange.start > 0 ? visibleRange.start - 1 : 0;
  const firstVisible = items[firstVisibleIndex];
  const headerLetter = getSectionLetter && firstVisible ? getSectionLetter(firstVisible) : null;
  const showRangePill = items.length > 0;

  if (error && items.length === 0) {
    return (
      <div className="p-4 text-[0.813rem] text-danger">
        {errorPrefix}: {error}
      </div>
    );
  }

  const listProps = {
    items,
    estimateSize,
    dynamicSize,
    selectedId,
    onSelect,
    isRowHighlighted,
    getId,
    getTextValue,
    renderRow,
    renderRowLead,
    requestMore,
    hasMore,
    onVisibleRangeChange: setVisibleRange,
    empty,
  };

  return (
    <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
      <ListRangeHeader
        rangeLabel={
          showRangePill ? undefined : loading && items.length === 0 ? rangeLabel : undefined
        }
        refreshing={!showRangePill && refreshing}
        filling={!showRangePill && filling}
        actions={headerActions}
        selectAllChecked={selectAllChecked}
        selectAllIndeterminate={selectAllIndeterminate}
        onSelectAllChange={onSelectAllChange}
        selectAllLabel={selectAllLabel}
        selectAllDisabled={items.length === 0}
      />
      {headerLetter ? (
        <div className="flex shrink-0 items-center border-b border-border bg-panel px-3 py-1">
          <span className="flex h-7 w-7 shrink-0 items-center justify-center text-[0.75rem] font-semibold text-muted">
            {headerLetter}
          </span>
        </div>
      ) : null}
      {getSectionLetter ? (
        <SectionedLetterList
          items={items}
          selectedId={selectedId}
          onSelect={onSelect}
          isRowHighlighted={isRowHighlighted}
          sectionLead={sectionLead}
          getId={getId}
          renderRow={renderRow}
          renderRowLead={renderRowLead}
          requestMore={requestMore}
          hasMore={hasMore}
          getSectionLetter={getSectionLetter}
          currentLetter={headerLetter}
          onVisibleRangeChange={setVisibleRange}
          empty={empty}
        />
      ) : isTauri() ? (
        <RacVirtualList {...listProps} ariaLabel={ariaLabel} />
      ) : (
        <TanStackVirtualList {...listProps} />
      )}
      {showRangePill ? (
        <ListRangePill
          rangeLabel={rangeLabel}
          refreshing={refreshing}
          filling={filling}
          testId="contact-list-range-pill"
        />
      ) : null}
    </div>
  );
}

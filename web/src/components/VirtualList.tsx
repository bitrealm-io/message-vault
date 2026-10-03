import { useVirtualizer, type VirtualItem } from "@tanstack/react-virtual";
import { type CSSProperties, type ReactNode, useEffect, useReducer, useRef } from "react";
import { resizeHandleGutter } from "../lib/tw";
import { useColumnResizing } from "./columnResizeState";

/** How often the x–y of z label may update while scrolling. */
const RANGE_REPORT_MS = 50;

export type VisibleRange = {
  /** 1-based inclusive index of the first row intersecting the viewport. */
  start: number;
  /** 1-based inclusive index of the last row intersecting the viewport. */
  end: number;
};

type VirtualListProps = {
  count: number;
  estimateSize?: number;
  /**
   * When false (default), every row uses `estimateSize` exactly — avoids scroll
   * drift from measureElement on uniform lists. Set true only when row heights vary.
   */
  dynamicSize?: boolean;
  overscan?: number;
  onVisibleRangeChange?: (range: VisibleRange) => void;
  /** Fired when the last visible row is within `nearEndThreshold` of the end. */
  onNearEnd?: () => void;
  /** 0-based distance from the last index that counts as “near end” (default 10). */
  nearEndThreshold?: number;
  renderItem: (index: number, virtualRow: VirtualItem) => ReactNode;
  style?: CSSProperties;
  empty?: ReactNode;
  footer?: ReactNode;
  /** Ignore this many CSS pixels at the bottom of the viewport when reporting the visible range. */
  visibleBottomInset?: number;
};

function rangeFromVirtualItems(
  virtualItems: VirtualItem[],
  scrollOffset: number,
  viewportHeight: number,
  count: number,
  bottomInset = 0,
): VisibleRange {
  if (count === 0 || virtualItems.length === 0 || viewportHeight <= 0) {
    return { start: 0, end: 0 };
  }

  const viewTop = scrollOffset;
  const viewBottom = scrollOffset + Math.max(0, viewportHeight - bottomInset);
  let startIdx: number | null = null;
  let endIdx: number | null = null;

  for (const row of virtualItems) {
    if (row.end > viewTop && row.start < viewBottom) {
      if (startIdx === null) startIdx = row.index;
      endIdx = row.index;
    }
  }

  if (startIdx === null || endIdx === null) return { start: 0, end: 0 };
  return { start: startIdx + 1, end: endIdx + 1 };
}

export default function VirtualList({
  count,
  estimateSize = 56,
  dynamicSize = false,
  overscan = 10,
  onVisibleRangeChange,
  onNearEnd,
  nearEndThreshold = 10,
  renderItem,
  style,
  empty,
  footer,
  visibleBottomInset = 0,
}: VirtualListProps) {
  const parentRef = useRef<HTMLDivElement | null>(null);
  const onRangeRef = useRef(onVisibleRangeChange);
  onRangeRef.current = onVisibleRangeChange;
  const onNearEndRef = useRef(onNearEnd);
  onNearEndRef.current = onNearEnd;
  const publishedRef = useRef<VisibleRange>({ start: 0, end: 0 });
  const pendingRef = useRef<VisibleRange | null>(null);
  const throttleTimerRef = useRef<number | null>(null);
  const lastScrollHeightRef = useRef<number | null>(null);
  // After layout or resize, re-read scroll size once the DOM has a height.
  const [, rerender] = useReducer((n: number) => n + 1, 0);

  // While the user drags the column width, skip per-row measurement so the list does not jump.
  const columnResizing = useColumnResizing();
  const measureRows = dynamicSize && !columnResizing;
  const wasResizingRef = useRef(false);

  const virtualizer = useVirtualizer({
    count,
    getScrollElement: () => parentRef.current,
    estimateSize: () => estimateSize,
    overscan,
  });

  // After a column-width drag ends, wait for wrap layout, then measure rows once.
  useEffect(() => {
    if (columnResizing) {
      wasResizingRef.current = true;
      return;
    }
    if (!dynamicSize || !wasResizingRef.current) return;
    wasResizingRef.current = false;

    let raf2 = 0;
    const raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(() => {
        virtualizer.measure();
        rerender();
      });
    });
    return () => {
      cancelAnimationFrame(raf1);
      if (raf2) cancelAnimationFrame(raf2);
    };
  }, [columnResizing, dynamicSize, virtualizer]);

  const virtualItems = virtualizer.getVirtualItems();
  const scrollOffset = parentRef.current?.scrollTop ?? 0;
  const viewportHeight = parentRef.current?.clientHeight ?? 0;
  const nextRange = rangeFromVirtualItems(
    virtualItems,
    scrollOffset,
    viewportHeight,
    count,
    visibleBottomInset,
  );
  const nextRangeStart = nextRange.start;
  const nextRangeEnd = nextRange.end;

  useEffect(() => {
    const prev = publishedRef.current;
    if (prev.start === nextRangeStart && prev.end === nextRangeEnd) {
      pendingRef.current = null;
      return;
    }

    const range: VisibleRange = { start: nextRangeStart, end: nextRangeEnd };

    const publish = (published: VisibleRange) => {
      publishedRef.current = published;
      onRangeRef.current?.(published);
      if (published.end >= 1 && count > 0 && published.end >= count - nearEndThreshold) {
        onNearEndRef.current?.();
      }
    };

    // First real measurement: publish immediately so the label is not stuck on "… of N".
    if (prev.start < 1 && nextRangeStart >= 1) {
      if (throttleTimerRef.current != null) {
        window.clearTimeout(throttleTimerRef.current);
        throttleTimerRef.current = null;
      }
      pendingRef.current = null;
      publish(range);
      return;
    }

    pendingRef.current = range;
    if (throttleTimerRef.current != null) return;

    throttleTimerRef.current = window.setTimeout(() => {
      throttleTimerRef.current = null;
      const pending = pendingRef.current;
      pendingRef.current = null;
      if (!pending) return;
      const last = publishedRef.current;
      if (last.start === pending.start && last.end === pending.end) return;
      publish(pending);
    }, RANGE_REPORT_MS);
  }, [nextRangeStart, nextRangeEnd, count, nearEndThreshold]);

  useEffect(() => {
    return () => {
      if (throttleTimerRef.current != null) {
        window.clearTimeout(throttleTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    const el = parentRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      // Width-only changes (dragging the list column) must not remeasure every pixel.
      // That is what made the conversation panel jitter. React only when height changes.
      const nextH = entries[0]?.contentRect.height ?? el.clientHeight;
      if (
        lastScrollHeightRef.current != null &&
        Math.abs(nextH - lastScrollHeightRef.current) < 0.5
      ) {
        return;
      }
      lastScrollHeightRef.current = nextH;
      if (columnResizing) return;
      virtualizer.measure();
      rerender();
    });
    ro.observe(el);
    // First paint often has height 0 until flex layout finishes.
    lastScrollHeightRef.current = el.clientHeight;
    rerender();
    return () => ro.disconnect();
    // `count` is deliberately not a dependency: the observed element does not
    // change when rows are appended, and re-running this called
    // `virtualizer.measure()`, throwing away every cached row measurement on a
    // dynamicSize list each time a page loaded.
  }, [virtualizer, columnResizing]);

  if (count === 0 && empty) {
    return (
      <div
        ref={parentRef}
        className={`min-h-0 flex-1 overflow-auto ${resizeHandleGutter}`}
        style={style}
      >
        {empty}
      </div>
    );
  }

  return (
    <div
      ref={parentRef}
      className={`min-h-0 flex-1 overflow-auto ${resizeHandleGutter}`}
      style={style}
    >
      <div
        style={{
          height: `${virtualizer.getTotalSize()}px`,
          width: "100%",
          position: "relative",
        }}
      >
        {virtualItems.map((virtualRow) => (
          <div
            key={virtualRow.key}
            data-index={virtualRow.index}
            ref={measureRows ? virtualizer.measureElement : undefined}
            style={{
              position: "absolute",
              top: 0,
              left: 0,
              width: "100%",
              height: measureRows ? undefined : `${virtualRow.size}px`,
              transform: `translateY(${virtualRow.start}px)`,
              boxSizing: "border-box",
            }}
          >
            {renderItem(virtualRow.index, virtualRow)}
          </div>
        ))}
      </div>
      {footer}
    </div>
  );
}

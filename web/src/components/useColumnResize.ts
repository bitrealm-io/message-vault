import type { KeyboardEvent, PointerEvent as ReactPointerEvent } from "react";
import { useEffect, useRef, useState } from "react";
import { clampWidth, loadWidth, saveWidth } from "./columnResize";

export type ColumnResizeHandleProps = {
  onPointerDown: (e: ReactPointerEvent<HTMLDivElement>) => void;
  onPointerMove: (e: ReactPointerEvent<HTMLDivElement>) => void;
  onPointerUp: (e: ReactPointerEvent<HTMLDivElement>) => void;
  onPointerCancel: (e: ReactPointerEvent<HTMLDivElement>) => void;
  onKeyDown: (e: KeyboardEvent<HTMLDivElement>) => void;
  onMouseEnter: () => void;
  onMouseLeave: () => void;
};

export type UseColumnResizeOptions = {
  storageKey: string;
  defaultWidth: number;
  minWidth: number;
  maxWidth: number;
  /** Called when a drag starts (true) or ends (false). */
  onDraggingChange?: (dragging: boolean) => void;
};

export type UseColumnResizeResult = {
  width: number;
  dragging: boolean;
  handleHover: boolean;
  handleProps: ColumnResizeHandleProps;
};

/** Clear body styles set for the duration of a column-width drag. */
function clearBodyDragStyles(): void {
  document.body.style.userSelect = "";
  document.body.style.cursor = "";
}

/**
 * On-screen width of the column that owns the resize handle.
 * Falls back to preferredWidth when the parent is missing (tests / detached nodes).
 */
export function measureColumnWidth(handle: HTMLElement, preferredWidth: number): number {
  const parent = handle.parentElement;
  if (!parent) return preferredWidth;
  const measured = parent.getBoundingClientRect().width;
  if (!Number.isFinite(measured) || measured <= 0) return preferredWidth;
  return measured;
}

/** Drag and keyboard resize for a vertical column, with localStorage persistence. */
export function useColumnResize({
  storageKey,
  defaultWidth,
  minWidth,
  maxWidth,
  onDraggingChange,
}: UseColumnResizeOptions): UseColumnResizeResult {
  const [width, setWidth] = useState(() => loadWidth(storageKey, defaultWidth, minWidth, maxWidth));
  const [dragging, setDragging] = useState(false);
  const [handleHover, setHandleHover] = useState(false);

  const startXRef = useRef(0);
  const startWidthRef = useRef(defaultWidth);
  const widthRef = useRef(width);
  widthRef.current = width;
  const draggingRef = useRef(false);
  const onDraggingChangeRef = useRef(onDraggingChange);
  onDraggingChangeRef.current = onDraggingChange;

  const setDraggingState = (next: boolean) => {
    draggingRef.current = next;
    setDragging(next);
    onDraggingChangeRef.current?.(next);
  };

  // If the column unmounts mid-drag (route change), clear body styles and context.
  useEffect(() => {
    return () => {
      if (!draggingRef.current) return;
      draggingRef.current = false;
      clearBodyDragStyles();
      onDraggingChangeRef.current?.(false);
    };
  }, []);

  const endDrag = (el: HTMLElement, pointerId: number) => {
    if (el.hasPointerCapture(pointerId)) {
      el.releasePointerCapture(pointerId);
    }
    setDraggingState(false);
    clearBodyDragStyles();
    saveWidth(storageKey, widthRef.current);
  };

  const onResizePointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    startXRef.current = e.clientX;
    // Use the painted width so a flex-shrunk column does not jump to preferred.
    startWidthRef.current = measureColumnWidth(e.currentTarget, widthRef.current);
    setDraggingState(true);
    document.body.style.userSelect = "none";
    document.body.style.cursor = "col-resize";
  };

  const onResizePointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!e.currentTarget.hasPointerCapture(e.pointerId)) return;
    const next = clampWidth(
      startWidthRef.current + (e.clientX - startXRef.current),
      minWidth,
      maxWidth,
    );
    widthRef.current = next;
    setWidth(next);
  };

  const onResizePointerUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    endDrag(e.currentTarget, e.pointerId);
  };

  const onResizeKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const step = e.shiftKey ? 24 : 8;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      // Start from the painted width, as a drag does, so a flex-shrunk column
      // moves on the first key press.
      const from = measureColumnWidth(e.currentTarget, widthRef.current);
      const next = clampWidth(
        e.key === "ArrowLeft" ? from - step : from + step,
        minWidth,
        maxWidth,
      );
      widthRef.current = next;
      setWidth(next);
      saveWidth(storageKey, next);
    } else if (e.key === "Home") {
      e.preventDefault();
      widthRef.current = minWidth;
      setWidth(minWidth);
      saveWidth(storageKey, minWidth);
    } else if (e.key === "End") {
      e.preventDefault();
      widthRef.current = maxWidth;
      setWidth(maxWidth);
      saveWidth(storageKey, maxWidth);
    }
  };

  return {
    width,
    dragging,
    handleHover,
    handleProps: {
      onPointerDown: onResizePointerDown,
      onPointerMove: onResizePointerMove,
      onPointerUp: onResizePointerUp,
      onPointerCancel: onResizePointerUp,
      onKeyDown: onResizeKeyDown,
      onMouseEnter: () => setHandleHover(true),
      onMouseLeave: () => setHandleHover(false),
    },
  };
}

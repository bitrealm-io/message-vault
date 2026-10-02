import { useLayoutEffect, useRef, useState } from "react";
import { Z_RESIZE_HANDLE } from "../lib/zLayers";
import { type ColumnResizeHandleProps, measureColumnWidth } from "./useColumnResize";

/** Vertical grip on the right edge of a resizable column. */
export default function ColumnResizeHandle({
  ariaLabel,
  width,
  minWidth,
  maxWidth,
  dragging,
  handleHover,
  handleProps,
}: {
  ariaLabel: string;
  width: number;
  minWidth: number;
  maxWidth: number;
  dragging: boolean;
  handleHover: boolean;
  handleProps: ColumnResizeHandleProps;
}) {
  const gripRef = useRef<HTMLDivElement>(null);
  // The width on screen, which a narrow window can squeeze below `width`.
  const [painted, setPainted] = useState(width);

  useLayoutEffect(() => {
    const grip = gripRef.current;
    if (!grip) return;
    const measure = () => setPainted(Math.round(measureColumnWidth(grip, width)));
    measure();
    const column = grip.parentElement;
    if (!column) return;
    const observer = new ResizeObserver(measure);
    observer.observe(column);
    return () => observer.disconnect();
  }, [width]);

  return (
    // biome-ignore lint/a11y/useSemanticElements: interactive column resize grip cannot use native hr
    <div
      ref={gripRef}
      role="separator"
      aria-orientation="vertical"
      aria-label={ariaLabel}
      aria-valuenow={painted}
      aria-valuemin={minWidth}
      aria-valuemax={maxWidth}
      tabIndex={0}
      {...handleProps}
      // w-2 matches `resizeHandleGutter`, the inset each resizable panel puts on its
      // scrolling child so this strip does not cover the scrollbar.
      className={`absolute top-0 right-0 h-full w-2 touch-none cursor-col-resize bg-transparent ${Z_RESIZE_HANDLE}`}
    >
      <div
        aria-hidden
        className={`pointer-events-none absolute top-0 right-0 bottom-0 w-px ${
          dragging || handleHover ? "bg-accent" : "bg-transparent"
        }`}
      />
    </div>
  );
}

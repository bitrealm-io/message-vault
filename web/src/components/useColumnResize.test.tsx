/** @vitest-environment jsdom */

import { act, renderHook } from "@testing-library/react";
import type { KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { measureColumnWidth, useColumnResize } from "./useColumnResize";

afterEach(() => {
  localStorage.clear();
  document.body.style.cursor = "";
  document.body.style.userSelect = "";
});

describe("measureColumnWidth", () => {
  it("reads the parent column width so a flex-shrunk drag starts from the screen", () => {
    const parent = document.createElement("div");
    const handle = document.createElement("div");
    parent.appendChild(handle);
    vi.spyOn(parent, "getBoundingClientRect").mockReturnValue({
      width: 80,
      height: 100,
      top: 0,
      left: 0,
      bottom: 100,
      right: 80,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });
    expect(measureColumnWidth(handle, 300)).toBe(80);
  });

  it("falls back to preferred width when detached", () => {
    const handle = document.createElement("div");
    expect(measureColumnWidth(handle, 300)).toBe(300);
  });
});

/** A handle inside a column the browser has painted at `painted` px wide. */
function handleInColumn(painted: number): HTMLDivElement {
  const parent = document.createElement("div");
  const handle = document.createElement("div");
  parent.appendChild(handle);
  vi.spyOn(parent, "getBoundingClientRect").mockReturnValue({
    width: painted,
    height: 100,
    top: 0,
    left: 0,
    bottom: 100,
    right: painted,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  });
  return handle;
}

describe("useColumnResize", () => {
  it("narrows a squeezed column from its painted width on one arrow key", () => {
    const { result } = renderHook(() =>
      useColumnResize({
        storageKey: "testCol:v1",
        defaultWidth: 400,
        minWidth: 160,
        maxWidth: 520,
      }),
    );

    act(() => {
      result.current.handleProps.onKeyDown({
        key: "ArrowLeft",
        shiftKey: false,
        preventDefault: () => {},
        currentTarget: handleInColumn(250),
      } as unknown as ReactKeyboardEvent<HTMLDivElement>);
    });

    expect(result.current.width).toBe(242);
  });

  it("clears body drag styles and reports false when unmounted mid-drag", () => {
    const onDraggingChange = vi.fn();
    const { result, unmount } = renderHook(() =>
      useColumnResize({
        storageKey: "testCol:v1",
        defaultWidth: 220,
        minWidth: 160,
        maxWidth: 360,
        onDraggingChange,
      }),
    );

    result.current.handleProps.onPointerDown({
      preventDefault: () => {},
      pointerId: 1,
      clientX: 100,
      currentTarget: {
        setPointerCapture: () => {},
        parentElement: null,
      },
    } as unknown as ReactPointerEvent<HTMLDivElement>);

    expect(onDraggingChange).toHaveBeenCalledWith(true);
    expect(document.body.style.cursor).toBe("col-resize");
    expect(document.body.style.userSelect).toBe("none");

    unmount();

    expect(onDraggingChange).toHaveBeenCalledWith(false);
    expect(document.body.style.cursor).toBe("");
    expect(document.body.style.userSelect).toBe("");
  });
});

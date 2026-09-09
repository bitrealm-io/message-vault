/** @vitest-environment jsdom */

import { fireEvent, render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ColumnResizeHandle from "./ColumnResizeHandle";

function handleProps() {
  return {
    onPointerDown: vi.fn(),
    onPointerMove: vi.fn(),
    onPointerUp: vi.fn(),
    onPointerCancel: vi.fn(),
    onKeyDown: vi.fn(),
    onMouseEnter: vi.fn(),
    onMouseLeave: vi.fn(),
  };
}

function renderHandle(props: ReturnType<typeof handleProps>) {
  return render(
    <ColumnResizeHandle
      ariaLabel="Resize navigation panel"
      width={220}
      minWidth={160}
      maxWidth={520}
      dragging={false}
      handleHover={false}
      handleProps={props}
    />,
  );
}

let props: ReturnType<typeof handleProps>;

beforeEach(() => {
  props = handleProps();
});

describe("ColumnResizeHandle", () => {
  it("keeps the grip on the inner right edge so the next column cannot cover it", () => {
    const { getByRole } = renderHandle(props);

    const handle = getByRole("separator", { name: "Resize navigation panel" });
    expect(handle.className).toContain("right-0");
    expect(handle.className).not.toContain("translate-x-full");
  });

  /**
   * The seven handlers `useColumnResize` supplies are the whole point of the
   * component: it renders a strip and forwards them. None was ever invoked, so
   * a version that spread `handleProps` onto the decorative inner div — or
   * dropped the spread entirely — rendered the same grip and passed. Dragging
   * a panel would then do nothing at all.
   */
  it("forwards every pointer handler to the grip a person actually drags", () => {
    const { getByRole } = renderHandle(props);
    const handle = getByRole("separator", { name: "Resize navigation panel" });

    fireEvent.pointerDown(handle);
    expect(props.onPointerDown).toHaveBeenCalledTimes(1);

    fireEvent.pointerMove(handle);
    expect(props.onPointerMove).toHaveBeenCalledTimes(1);

    fireEvent.pointerUp(handle);
    expect(props.onPointerUp).toHaveBeenCalledTimes(1);

    fireEvent.pointerCancel(handle);
    expect(props.onPointerCancel).toHaveBeenCalledTimes(1);
  });

  it("forwards the hover handlers, which is what draws the accent line", async () => {
    const user = userEvent.setup();
    const { getByRole } = renderHandle(props);
    const handle = getByRole("separator", { name: "Resize navigation panel" });

    await user.hover(handle);
    expect(props.onMouseEnter).toHaveBeenCalledTimes(1);

    await user.unhover(handle);
    expect(props.onMouseLeave).toHaveBeenCalledTimes(1);
  });

  it("takes focus and forwards keys, so the column can be resized without a mouse", async () => {
    const user = userEvent.setup();
    const { getByRole } = renderHandle(props);
    const handle = getByRole("separator", { name: "Resize navigation panel" });

    await user.tab();
    expect(handle).toHaveFocus();

    await user.keyboard("{ArrowRight}");
    expect(props.onKeyDown).toHaveBeenCalled();
    expect(props.onKeyDown.mock.calls.at(-1)?.[0]).toMatchObject({ key: "ArrowRight" });
  });

  it("reports the width it is at and the range it may move in, for a screen reader", () => {
    const { getByRole } = renderHandle(props);
    const handle = getByRole("separator", { name: "Resize navigation panel" });

    expect(handle).toHaveAttribute("aria-valuenow", "220");
    expect(handle).toHaveAttribute("aria-valuemin", "160");
    expect(handle).toHaveAttribute("aria-valuemax", "520");
    expect(handle).toHaveAttribute("aria-orientation", "vertical");
  });
});

/** @vitest-environment jsdom */

import { act, cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter, useLocation } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { isTauri } from "../lib/tauri-check";
import { useMouseHistoryNavigation } from "./useMouseHistoryNavigation";

vi.mock("../lib/tauri-check", () => ({ isTauri: vi.fn() }));

const isTauriMock = vi.mocked(isTauri);

const BACK_BUTTON = 3;
const FORWARD_BUTTON = 4;

function Probe() {
  useMouseHistoryNavigation();
  return <output>{useLocation().pathname}</output>;
}

/** Two entries deep, sitting on the second, so back and forward both have somewhere to go. */
function renderProbe() {
  return render(
    <MemoryRouter initialEntries={["/first", "/second"]} initialIndex={1}>
      <Probe />
    </MemoryRouter>,
  );
}

/** Dispatch the way a real click does: from an element, up through the tree. */
function clickButton(button: number): MouseEvent {
  const event = new MouseEvent("mousedown", {
    button,
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    document.body.dispatchEvent(event);
  });
  return event;
}

function path(): string {
  return screen.getByRole("status").textContent ?? "";
}

beforeEach(() => {
  isTauriMock.mockReturnValue(true);
});

afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

describe("useMouseHistoryNavigation", () => {
  it("goes back one entry on the thumb back button", () => {
    renderProbe();
    expect(path()).toBe("/second");
    clickButton(BACK_BUTTON);
    expect(path()).toBe("/first");
  });

  it("goes forward one entry on the thumb forward button", () => {
    renderProbe();
    clickButton(BACK_BUTTON);
    expect(path()).toBe("/first");
    clickButton(FORWARD_BUTTON);
    expect(path()).toBe("/second");
  });

  it("cancels the event so a webview that maps these buttons cannot step twice", () => {
    renderProbe();
    expect(clickButton(BACK_BUTTON).defaultPrevented).toBe(true);
  });

  it("leaves ordinary mouse buttons alone", () => {
    renderProbe();
    const left = clickButton(0);
    const middle = clickButton(1);
    expect(path()).toBe("/second");
    expect(left.defaultPrevented).toBe(false);
    expect(middle.defaultPrevented).toBe(false);
  });

  it("stays out of the way in a browser, where these buttons already work", () => {
    isTauriMock.mockReturnValue(false);
    renderProbe();
    const event = clickButton(BACK_BUTTON);
    expect(path()).toBe("/second");
    expect(event.defaultPrevented).toBe(false);
  });

  it("drops its listeners on unmount", () => {
    const { unmount } = renderProbe();
    unmount();
    expect(clickButton(BACK_BUTTON).defaultPrevented).toBe(false);
  });
});

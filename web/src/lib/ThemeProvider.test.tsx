/** @vitest-environment jsdom */

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ThemeProvider, useTheme } from "./ThemeProvider";

describe("ThemeProvider with browser storage blocked", () => {
  beforeEach(() => {
    // jsdom has no matchMedia, and the theme reads the system colour scheme from it.
    vi.stubGlobal("matchMedia", () => ({
      matches: false,
      addEventListener: () => {},
      removeEventListener: () => {},
    }));
    // A browser that blocks site data throws SecurityError from the
    // localStorage getter itself, for reads and writes alike.
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      get() {
        throw new DOMException("The operation is insecure.", "SecurityError");
      },
    });
  });

  afterEach(() => {
    // The own property shadows jsdom's getter on the prototype.
    Reflect.deleteProperty(window, "localStorage");
    vi.unstubAllGlobals();
    document.documentElement.removeAttribute("data-theme");
  });

  it("opens with the default theme and applies a change for the visit", () => {
    const { result } = renderHook(() => useTheme(), { wrapper: ThemeProvider });
    expect(result.current.mode).toBe("dark");
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");

    act(() => {
      result.current.setMode("light");
    });

    expect(result.current.mode).toBe("light");
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
  });
});

/** @vitest-environment jsdom */
import { describe, expect, it } from "vitest";
import { shouldIgnoreOutsideDismiss } from "./portaledOverlay";

function clickEvent(path: EventTarget[]): MouseEvent {
  const event = new MouseEvent("mousedown", { bubbles: true });
  Object.defineProperty(event, "composedPath", {
    value: () => path,
  });
  return event;
}

describe("shouldIgnoreOutsideDismiss", () => {
  it("ignores when root is null", () => {
    expect(shouldIgnoreOutsideDismiss(clickEvent([]), null)).toBe(true);
  });

  it("ignores clicks inside the panel root", () => {
    const root = document.createElement("div");
    const child = document.createElement("button");
    root.appendChild(child);
    document.body.appendChild(root);
    expect(shouldIgnoreOutsideDismiss(clickEvent([child, root]), root)).toBe(true);
    root.remove();
  });

  it("ignores clicks on marked portaled overlays", () => {
    const root = document.createElement("div");
    const overlay = document.createElement("div");
    overlay.setAttribute("data-mc-overlay", "");
    document.body.append(root, overlay);
    expect(shouldIgnoreOutsideDismiss(clickEvent([overlay]), root)).toBe(true);
    root.remove();
    overlay.remove();
  });

  it("does not ignore true outside clicks", () => {
    const root = document.createElement("div");
    const outside = document.createElement("div");
    document.body.append(root, outside);
    expect(shouldIgnoreOutsideDismiss(clickEvent([outside]), root)).toBe(false);
    root.remove();
    outside.remove();
  });
});

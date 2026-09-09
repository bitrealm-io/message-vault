import { afterEach } from "vitest";
import "@testing-library/jest-dom/vitest";

// jsdom has no Web Animations API. React Aria's SelectionIndicator (the sliding
// underline under our tab strips) calls getAnimations() on mount, so without
// this stub any test that renders Tabs throws instead of rendering.
if (typeof Element !== "undefined" && !Element.prototype.getAnimations) {
  Element.prototype.getAnimations = () => [];
}

// Testing Library unmounts what a test rendered in a global `afterEach`, which
// it registers only when Vitest's globals are on. They are off here, so
// without this every render in a file stacks up in the same document and a
// query that should find one element finds one per test that has run. The
// import is inside the hook so a test in the node environment, which never
// renders anything, does not pull in react-dom.
afterEach(async () => {
  if (typeof document === "undefined") return;
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

// jsdom has no ResizeObserver. React Aria's Virtualizer observes its scroller
// on mount, so without this any test rendering a virtualized list throws.
if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
}

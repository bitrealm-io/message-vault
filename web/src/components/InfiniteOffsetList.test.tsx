/** @vitest-environment jsdom */

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import InfiniteOffsetList from "./InfiniteOffsetList";

vi.mock("../lib/tauri-check", () => ({
  isTauri: () => false,
}));

afterEach(() => {
  cleanup();
});

type Item = { id: string; name: string };

function renderList(
  items: Item[],
  extra?: {
    loading?: boolean;
    filling?: boolean;
    hasMore?: boolean;
    requestMore?: () => void;
    onSelect?: (item: Item) => void;
    sectioned?: boolean;
  },
) {
  return render(
    <div style={{ height: 400 }}>
      <InfiniteOffsetList
        items={items}
        total={items.length}
        loading={extra?.loading ?? false}
        filling={extra?.filling ?? false}
        error=""
        hasMore={extra?.hasMore ?? false}
        requestMore={extra?.requestMore ?? (() => {})}
        estimateSize={49}
        getId={(c) => c.id}
        onSelect={extra?.onSelect ?? (() => {})}
        onSelectAllChange={() => {}}
        selectAllLabel="Select all contacts"
        renderRow={(c) => <span>{c.name}</span>}
        ariaLabel="Contacts"
        getSectionLetter={
          extra?.sectioned === false ? undefined : (c) => c.name.charAt(0).toUpperCase()
        }
      />
    </div>,
  );
}

describe("InfiniteOffsetList range pill", () => {
  it("shows the floating range pill outside the toolbar", () => {
    renderList([
      { id: "1", name: "Alice" },
      { id: "2", name: "Bob" },
    ]);
    const pill = screen.getByTestId("contact-list-range-pill");
    expect(pill).toHaveTextContent("of 2");
    expect(pill).not.toHaveAttribute("aria-live");

    const toolbar = screen.getByRole("checkbox", { name: "Select all contacts" }).closest("div");
    expect(toolbar).toBeTruthy();
    expect(within(toolbar as HTMLElement).queryByText(/of 2/)).not.toBeInTheDocument();
  });

  it("appends loading-more on the pill, not the toolbar", () => {
    renderList(
      [
        { id: "1", name: "Alice" },
        { id: "2", name: "Bob" },
      ],
      { filling: true },
    );
    const pill = screen.getByTestId("contact-list-range-pill");
    expect(pill).toHaveTextContent(/loading more/);
    const toolbar = screen.getByRole("checkbox", { name: "Select all contacts" }).closest("div");
    expect(within(toolbar as HTMLElement).queryByText(/loading more/)).not.toBeInTheDocument();
  });

  it("keeps Loading… in the toolbar when the list is still empty", () => {
    renderList([], { loading: true });
    expect(screen.queryByTestId("contact-list-range-pill")).not.toBeInTheDocument();
    expect(screen.getByText("Loading…")).toBeInTheDocument();
  });

  it("hides the range pill when the list is empty", () => {
    renderList([]);
    expect(screen.queryByTestId("contact-list-range-pill")).not.toBeInTheDocument();
    expect(screen.queryByText("Loading…")).not.toBeInTheDocument();
  });
});

/** Fifty rows, enough that the near-end threshold is a real boundary. */
function manyItems(): Item[] {
  return Array.from({ length: 50 }, (_, i) => ({ id: String(i), name: `Person ${i}` }));
}

/**
 * jsdom lays nothing out, so every `getBoundingClientRect` is zeros and the
 * sectioned list sees no rows on screen. This gives the scroller a 400px
 * viewport and stacks the rows 49px apart from `scrollTop`, which is the
 * geometry the component reads.
 */
function layOutRows(scroller: HTMLElement, scrollTop: number, viewport = 400) {
  Object.defineProperty(scroller, "scrollTop", { value: scrollTop, configurable: true });
  Object.defineProperty(scroller, "clientHeight", { value: viewport, configurable: true });
  scroller.getBoundingClientRect = () =>
    ({ top: 0, bottom: viewport, left: 0, right: 0, width: 0, height: viewport }) as DOMRect;
  for (const row of scroller.querySelectorAll<HTMLElement>("[data-contact-index]")) {
    const index = Number(row.getAttribute("data-contact-index"));
    const top = index * 49 - scrollTop;
    row.getBoundingClientRect = () =>
      ({ top, bottom: top + 49, left: 0, right: 0, width: 0, height: 49 }) as DOMRect;
  }
}

/** The scrolling element of whichever list path is rendered. */
function scroller(container: HTMLElement): HTMLElement {
  const el = container.querySelector<HTMLElement>(".overflow-auto");
  if (!el) throw new Error("no scroller rendered");
  return el;
}

/**
 * The callback the whole component exists for.
 *
 * `requestMore` is how the list asks the vault for the next page, and every
 * test in this file passed `hasMore={false}` and a no-op, so it was never
 * called. A list that had stopped asking — one showing the first forty
 * contacts and nothing more however far you scrolled — passed all of them.
 */
describe("InfiniteOffsetList asking for more", () => {
  it("asks for the next page once the last rows come into view", async () => {
    const requestMore = vi.fn();
    const { container } = renderList(manyItems(), { hasMore: true, requestMore });
    const root = scroller(container);

    layOutRows(root, 42 * 49);
    fireEvent.scroll(root);
    await waitFor(() => expect(requestMore).toHaveBeenCalled());
  });

  it("does not ask while the top of a long list is on screen", async () => {
    const requestMore = vi.fn();
    const { container } = renderList(manyItems(), { hasMore: true, requestMore });
    const root = scroller(container);

    layOutRows(root, 0);
    fireEvent.scroll(root);
    await new Promise((resolve) => requestAnimationFrame(() => resolve(null)));

    expect(requestMore).not.toHaveBeenCalled();
  });

  it("does not ask when the vault has already sent everything", async () => {
    const requestMore = vi.fn();
    const { container } = renderList(manyItems(), { hasMore: false, requestMore });
    const root = scroller(container);

    layOutRows(root, 42 * 49);
    fireEvent.scroll(root);
    await new Promise((resolve) => requestAnimationFrame(() => resolve(null)));

    expect(requestMore).not.toHaveBeenCalled();
  });

  it("does not ask about an empty list, which would page forever after a failed load", async () => {
    const requestMore = vi.fn();
    renderList([], { hasMore: true, requestMore });

    await new Promise((resolve) => requestAnimationFrame(() => resolve(null)));

    expect(requestMore).not.toHaveBeenCalled();
  });

  // Only the sectioned path is exercised here. The virtualized path builds its
  // range from `rangeFromScroll`, and React Aria's Virtualizer does not lay
  // out or forward scroll in jsdom, so a test of it would assert that the
  // harness is wired rather than that the list asks for more. The contact and
  // conversation lists both pass `getSectionLetter`, so the path covered above
  // is the one that runs.
});

describe("InfiniteOffsetList choosing a row", () => {
  it("hands the item back when its row is clicked", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    renderList(
      [
        { id: "1", name: "Alice" },
        { id: "2", name: "Bob" },
      ],
      { onSelect },
    );

    await user.click(screen.getByText("Bob"));

    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledWith({ id: "2", name: "Bob" });
  });
});

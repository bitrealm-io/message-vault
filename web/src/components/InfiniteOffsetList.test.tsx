/** @vitest-environment jsdom */

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import InfiniteOffsetList from "./InfiniteOffsetList";

const tauriMock = vi.hoisted(() => ({ current: false }));
vi.mock("../lib/tauri-check", () => ({
  isTauri: () => tauriMock.current,
}));

afterEach(() => {
  cleanup();
  tauriMock.current = false;
});

describe("InfiniteOffsetList in the desktop app", () => {
  it("opens a search result on one click", async () => {
    tauriMock.current = true;
    // jsdom lays out nothing; give the virtualizer a viewport to fill.
    const heights = vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(400);
    const widths = vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(300);
    try {
      const onSelect = vi.fn();
      renderList(
        [
          { id: "1", name: "Ada" },
          { id: "2", name: "Grace" },
        ],
        { onSelect, sectioned: false },
      );
      await userEvent.click(await screen.findByText("Grace"));
      expect(onSelect).toHaveBeenCalledTimes(1);
      expect(onSelect).toHaveBeenCalledWith({ id: "2", name: "Grace" });
    } finally {
      heights.mockRestore();
      widths.mockRestore();
    }
  });

  it("shows the rows on screen and asks for more when the first page fits, without a scroll", async () => {
    tauriMock.current = true;
    const requestMore = vi.fn();
    const restore = layOutDrawnRows(49);
    try {
      renderList(manyItems().slice(0, 6), {
        sectioned: false,
        hasMore: true,
        requestMore,
        total: 90,
      });
      const pill = screen.getByTestId("contact-list-range-pill");
      await waitFor(() => expect(pill).toHaveTextContent("1–6 of 90"));
      expect(requestMore).toHaveBeenCalled();
    } finally {
      restore();
    }
  });

  it("reads the range from the rows' own heights, not the estimate", async () => {
    tauriMock.current = true;
    const requestMore = vi.fn();
    // Rows twice the estimate: four of them reach into the 360px above the pill.
    const restore = layOutDrawnRows(98);
    try {
      renderList(manyItems(), { sectioned: false, hasMore: true, requestMore, total: 90 });
      const pill = screen.getByTestId("contact-list-range-pill");
      await waitFor(() => expect(pill).toHaveTextContent("1–4 of 90"));
      expect(requestMore).not.toHaveBeenCalled();
    } finally {
      restore();
    }
  });

  it("works the range out again when a new search replaces the rows", async () => {
    tauriMock.current = true;
    const restore = layOutDrawnRows(49);
    try {
      const { rerender } = renderList(manyItems(), { sectioned: false, total: 90 });
      const pill = screen.getByTestId("contact-list-range-pill");
      await waitFor(() => expect(pill).toHaveTextContent("1–8 of 90"));

      rerender(listElement(manyItems().slice(0, 3), { sectioned: false, total: 3 }));
      await waitFor(() => expect(pill).toHaveTextContent("1–3 of 3"));
    } finally {
      restore();
    }
  });
});

/**
 * jsdom lays out nothing. This gives every element a 400px viewport and puts
 * each row React Aria's Virtualizer draws at its place in the list, `height`
 * pixels apart, whatever height the virtualizer itself assumed.
 */
function layOutDrawnRows(height: number) {
  const viewport = 400;
  const heights = vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(viewport);
  const widths = vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(300);
  const rects = vi
    .spyOn(HTMLElement.prototype, "getBoundingClientRect")
    .mockImplementation(function (this: HTMLElement) {
      const position = Number(this.getAttribute("aria-posinset"));
      if (this.getAttribute("role") === "option" && position > 0) {
        const listbox = this.closest<HTMLElement>('[role="listbox"]');
        const top = (position - 1) * height - (listbox?.scrollTop ?? 0);
        return { top, bottom: top + height, left: 0, right: 300, width: 300, height } as DOMRect;
      }
      return {
        top: 0,
        bottom: viewport,
        left: 0,
        right: 300,
        width: 300,
        height: viewport,
      } as DOMRect;
    });
  return () => {
    heights.mockRestore();
    widths.mockRestore();
    rects.mockRestore();
  };
}

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
    lead?: boolean;
    total?: number;
  },
) {
  return render(listElement(items, extra));
}

function listElement(items: Item[], extra?: Parameters<typeof renderList>[1]) {
  return (
    <div style={{ height: 400 }}>
      <InfiniteOffsetList
        items={items}
        total={extra?.total ?? items.length}
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
        renderRowLead={
          extra?.lead ? (c) => <input type="checkbox" aria-label={`Select ${c.name}`} /> : undefined
        }
        ariaLabel="Contacts"
        getSectionLetter={
          extra?.sectioned === false ? undefined : (c) => c.name.charAt(0).toUpperCase()
        }
      />
    </div>
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
 * `requestMore` is how the list asks the server for the next page, and every
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

  it("does not ask when the server has already sent everything", async () => {
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

  // Only the sectioned path is exercised here. The desktop app's virtualized
  // path, used for a contact search and the "Last heard" sort, has its own
  // tests under "InfiniteOffsetList in the desktop app".
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

  // jsdom lays nothing out, so this guards the structure and the browser check
  // guards the pixels: the button that selects has to cover the whole row, or
  // the padding around the name highlights on hover and then ignores the click.
  it("stretches the select button over the whole row when a lead cell sits beside it", () => {
    renderList([{ id: "1", name: "Alice" }], { lead: true });

    const button = screen.getByRole("button", { name: "Alice" });
    const row = button.parentElement as HTMLElement;
    expect(row.className).toContain("relative");
    expect(button.className).toContain("after:absolute");
    expect(button.className).toContain("after:inset-0");

    // The lead cell has to sit above that stretched target to stay clickable.
    const lead = screen.getByRole("checkbox", { name: "Select Alice" })
      .parentElement as HTMLElement;
    expect(lead.className).toContain("relative");
    expect(lead.className).toContain("z-[1]");
  });

  it("selects from the row and not from the lead cell", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    renderList([{ id: "1", name: "Alice" }], { onSelect, lead: true });

    await user.click(screen.getByRole("button", { name: "Alice" }));
    expect(onSelect).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("checkbox", { name: "Select Alice" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
  });
});

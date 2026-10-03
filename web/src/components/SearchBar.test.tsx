/** @vitest-environment jsdom */

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { type ComponentProps, useRef, useState } from "react";
import { MemoryRouter, useLocation, useSearchParams } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import SearchBar from "./SearchBar";

const recentsMock = vi.hoisted(() => ({ current: ["ada", "grace"] as string[] }));

vi.mock("../lib/recentSearches", () => ({
  loadRecentSearches: () => recentsMock.current,
  // As the real one does: the query moves to the front.
  pushRecentSearch: vi.fn((_scope: string, q: string) => {
    recentsMock.current = [q, ...recentsMock.current.filter((x) => x !== q)];
    return recentsMock.current;
  }),
  clearRecentSearches: vi.fn(),
}));

// The advanced panel pulls in the whole filter form; the combobox is what is under test.
vi.mock("./AdvancedSearchForm", () => ({
  default: () => <div data-testid="advanced-form" />,
}));

const suggestionsMock = vi.hoisted(() => ({
  current: [] as { id: string; label: string; insert: string }[],
}));

// Only the hook is faked. `applySuggestionToQuery` is the real one: the mock
// used to reimplement it line for line, so the test proved the copy worked
// and would have passed with the product function deleted.
vi.mock("../lib/useSearchSuggestions", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/useSearchSuggestions")>()),
  useSearchSuggestions: () => suggestionsMock.current,
}));

function renderSearch(props: Partial<ComponentProps<typeof SearchBar>> = {}) {
  const onSubmit = props.onSubmit ?? vi.fn();
  const onChange = props.onChange ?? vi.fn();
  const placeholder = props.placeholder ?? "Search contacts";
  render(
    <SearchBar
      value={props.value ?? ""}
      onChange={onChange}
      onSubmit={onSubmit}
      scope={props.scope ?? "contact"}
      list={props.list ?? "contacts"}
      placeholder={placeholder}
      advancedMode={props.advancedMode ?? "contacts"}
    />,
  );
  return { onSubmit, onChange, input: screen.getByRole("combobox", { name: placeholder }) };
}

/** The Messages box's fixed props, as the header passes them. */
const messageBox = {
  scope: "message",
  list: null,
  placeholder: "Search messages",
  advancedMode: null,
} as const;

/** Buttons that set the parent's value from outside, as a Saved Search does. */
function SavedSearches({ setValue }: { setValue: (q: string) => void }) {
  return (
    <>
      <button type="button" onClick={() => setValue("kind:group")}>
        Open a Saved Search
      </button>
      <button type="button" onClick={() => setValue("a")}>
        Open a Saved Search for a
      </button>
    </>
  );
}

/**
 * The bar with a parent that takes its time: each change reaches `value` on a
 * later task, as the address does in the browser, where React Router applies
 * the change as a transition after the box has drawn its old value again.
 */
function LaggingSearch({ onSubmit }: { onSubmit: (q: string) => void }) {
  const [value, setValue] = useState("");
  return (
    <>
      <SearchBar
        {...messageBox}
        value={value}
        onChange={(q) => {
          setTimeout(() => setValue(q), 0);
        }}
        onSubmit={onSubmit}
      />
      <output data-testid="value">{value}</output>
      <SavedSearches setValue={setValue} />
    </>
  );
}

/**
 * A late parent that applies the changes arriving together as the last one,
 * as the router can: `a` then Backspace leaves its value at "" with no echo.
 */
function MergingSearch({ onSubmit }: { onSubmit: (q: string) => void }) {
  const [value, setValue] = useState("");
  const latest = useRef<string | null>(null);
  return (
    <>
      <SearchBar
        {...messageBox}
        value={value}
        onChange={(q) => {
          if (latest.current === null) {
            setTimeout(() => {
              setValue(latest.current ?? "");
              latest.current = null;
            }, 0);
          }
          latest.current = q;
        }}
        onSubmit={onSubmit}
      />
      <SavedSearches setValue={setValue} />
    </>
  );
}

function renderLateSearch(Parent: typeof LaggingSearch) {
  const onSubmit = vi.fn();
  render(<Parent onSubmit={onSubmit} />);
  return { onSubmit, input: screen.getByRole("combobox", { name: "Search messages" }) };
}

/** The bar as `AppLayout` wires the Messages box: its value is the address's `q`, written with `replace`. */
function AddressSearch({ onSubmit }: { onSubmit: (q: string) => void }) {
  const [params, setParams] = useSearchParams();
  const location = useLocation();
  return (
    <>
      <SearchBar
        {...messageBox}
        value={params.get("q") ?? ""}
        onChange={(q) => setParams(q ? { q } : {}, { replace: true })}
        onSubmit={onSubmit}
      />
      <output data-testid="location">{location.search}</output>
    </>
  );
}

describe("SearchBar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    suggestionsMock.current = [];
    recentsMock.current = ["ada", "grace"];
  });

  afterEach(() => {
    cleanup();
  });

  it("walks the recent searches with arrow keys and reports the active row", async () => {
    const user = userEvent.setup();
    const { input } = renderSearch();

    await user.click(input);
    expect(input.getAttribute("aria-activedescendant")).toBeNull();

    await user.keyboard("{ArrowDown}");
    const [first] = screen.getAllByRole("option");
    // `aria-activedescendant` tells a screen reader which row is active. The
    // row is not selected, because every row is an action and none is the value.
    expect(input.getAttribute("aria-activedescendant")).toBe(first.id);

    await user.keyboard("{ArrowDown}");
    const second = screen.getAllByRole("option")[1];
    expect(input.getAttribute("aria-activedescendant")).toBe(second.id);
  });

  it("submits the highlighted recent search on Enter", async () => {
    const user = userEvent.setup();
    const { input, onSubmit } = renderSearch();

    await user.click(input);
    await user.keyboard("{ArrowDown}{Enter}");

    expect(onSubmit).toHaveBeenCalledWith("ada");
  });

  it("runs the query the box shows when Enter is pressed a second time", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    function Controlled() {
      const [value, setValue] = useState("");
      return (
        <SearchBar
          value={value}
          onChange={setValue}
          onSubmit={onSubmit}
          scope="contact"
          list="contacts"
          placeholder="Search contacts"
          advancedMode="contacts"
        />
      );
    }
    render(<Controlled />);
    const input = screen.getByRole("combobox", { name: "Search contacts" });

    await user.click(input);
    await user.keyboard("{ArrowDown}{ArrowDown}{Enter}");
    expect(onSubmit).toHaveBeenLastCalledWith("grace");
    expect(input).toHaveValue("grace");

    await user.keyboard("{Enter}");
    expect(onSubmit).toHaveBeenLastCalledWith("grace");
  });

  it("runs a search typed after a row was clicked on the first Enter", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    function Controlled() {
      const [value, setValue] = useState("");
      return (
        <SearchBar
          value={value}
          onChange={setValue}
          onSubmit={onSubmit}
          scope="contact"
          list="contacts"
          placeholder="Search contacts"
          advancedMode="contacts"
        />
      );
    }
    render(<Controlled />);
    const input = screen.getByRole("combobox", { name: "Search contacts" });

    await user.click(input);
    await user.click(screen.getByRole("option", { name: "ada" }));
    expect(onSubmit).toHaveBeenLastCalledWith("ada");
    await user.clear(input);
    await user.type(input, "bob");
    await user.keyboard("{Escape}{Enter}");

    expect(onSubmit).toHaveBeenLastCalledWith("bob");
  });

  it("submits the typed text when no row is highlighted", async () => {
    const user = userEvent.setup();
    const { input, onSubmit } = renderSearch({ value: "typed" });

    await user.click(input);
    await user.keyboard("{Enter}");

    expect(onSubmit).toHaveBeenCalledWith("typed");
  });

  it("reaches the advanced-search row by keyboard", async () => {
    const user = userEvent.setup();
    const { input } = renderSearch();

    await user.click(input);
    // Two recents, then the advanced row.
    await user.keyboard("{ArrowDown}{ArrowDown}{ArrowDown}{Enter}");

    expect(await screen.findByTestId("advanced-form")).toBeTruthy();
  });

  it("keeps the advanced panel open, with no popdown over it, when the box is focused again", async () => {
    const user = userEvent.setup();
    const { input } = renderSearch();

    await user.click(input);
    await user.keyboard("{ArrowUp}{Enter}");
    expect(await screen.findByTestId("advanced-form")).toBeTruthy();

    act(() => input.blur());
    await user.click(input);

    expect(screen.getByTestId("advanced-form")).toBeTruthy();
    expect(screen.queryAllByRole("option")).toHaveLength(0);
  });

  it("wraps from the last row back to the first", async () => {
    const user = userEvent.setup();
    const { input } = renderSearch();

    await user.click(input);
    const optionIds = () => screen.getAllByRole("option").map((el) => el.id);

    // Three rows: two recents plus advanced. A fourth press wraps.
    await user.keyboard("{ArrowDown}{ArrowDown}{ArrowDown}{ArrowDown}");
    expect(input.getAttribute("aria-activedescendant")).toBe(optionIds()[0]);
  });

  it("opens on the last row with the up arrow", async () => {
    const user = userEvent.setup();
    const { input } = renderSearch();

    await user.click(input);
    await user.keyboard("{Escape}");
    expect(screen.queryAllByRole("option")).toHaveLength(0);

    await user.keyboard("{ArrowUp}");
    const options = screen.getAllByRole("option");
    expect(input.getAttribute("aria-activedescendant")).toBe(options[options.length - 1]?.id);
    expect(options[options.length - 1]).toHaveTextContent("Advanced search");
  });

  it("closes the popdown on Escape", async () => {
    const user = userEvent.setup();
    const { input } = renderSearch();

    await user.click(input);
    expect(screen.getAllByRole("option").length).toBeGreaterThan(0);

    await user.keyboard("{Escape}");
    expect(screen.queryAllByRole("option")).toHaveLength(0);
  });

  it("labels each bar with its own placeholder", () => {
    renderSearch({ scope: "trash", placeholder: "Search Trash", advancedMode: "messages" });
    expect(screen.getByRole("combobox", { name: "Search Trash" })).toBeTruthy();
  });

  it("namespaces row ids per scope so two bars never collide", async () => {
    const user = userEvent.setup();
    const { input } = renderSearch({ scope: "message", placeholder: "Search messages" });

    await user.click(input);
    for (const option of screen.getAllByRole("option")) {
      expect(option.id.startsWith("message-search-")).toBe(true);
    }
  });

  it("shows word autocomplete instead of recents while a token is being typed", async () => {
    suggestionsMock.current = [{ id: "identity:", label: "identity:", insert: "identity: " }];
    const user = userEvent.setup();
    const { input } = renderSearch({
      value: "ide",
      scope: "message",
      placeholder: "Search messages",
      advancedMode: "messages",
    });

    await user.click(input);
    const labels = screen.getAllByRole("option").map((el) => el.textContent);
    expect(labels).toEqual(["identity:"]);
  });

  it("inserts a suggestion into the query without running the search", async () => {
    suggestionsMock.current = [{ id: "identity:", label: "identity:", insert: "identity: " }];
    const user = userEvent.setup();
    const { input, onChange, onSubmit } = renderSearch({
      value: "ide",
      scope: "message",
      placeholder: "Search messages",
      advancedMode: "messages",
    });

    await user.click(input);
    await user.keyboard("{ArrowDown}{Enter}");

    expect(onChange).toHaveBeenCalledWith("identity: ");
    expect(onSubmit).not.toHaveBeenCalled();
  });

  describe("with a value that arrives late (#1000)", () => {
    beforeEach(() => {
      recentsMock.current = [];
    });

    it("searches for a paste followed at once by Enter", async () => {
      const user = userEvent.setup({ delay: null });
      const { onSubmit, input } = renderLateSearch(LaggingSearch);

      await user.click(input);
      // A paste is one input event, and Enter can follow it before the
      // parent's value has caught up.
      act(() => {
        fireEvent.change(input, { target: { value: "attachment:any" } });
        fireEvent.keyDown(input, { key: "Enter" });
      });

      expect(onSubmit).toHaveBeenLastCalledWith("attachment:any");
    });

    it("keeps every key of text typed with no delay", async () => {
      const user = userEvent.setup({ delay: null });
      const { input } = renderLateSearch(LaggingSearch);

      await user.type(input, "attachment:any");

      expect(input).toHaveValue("attachment:any");
      await act(() => new Promise((resolve) => setTimeout(resolve, 0)));
      expect(screen.getByTestId("value").textContent).toBe("attachment:any");
      expect(input).toHaveValue("attachment:any");
    });

    it("shows a search set from outside the box, such as a Saved Search", async () => {
      const user = userEvent.setup();
      const { input } = renderLateSearch(LaggingSearch);

      await user.type(input, "ada");
      await act(() => new Promise((resolve) => setTimeout(resolve, 0)));
      await user.click(screen.getByRole("button", { name: "Open a Saved Search" }));

      expect(input).toHaveValue("kind:group");
    });

    it("shows a search set from outside after changes the parent merged into none", async () => {
      // The router can apply several changes as one: `a` then Backspace
      // leaves the address at "" and nothing is echoed. A Saved Search for
      // `a` afterwards is the person's choice, not an echo of the `a` typed.
      const user = userEvent.setup({ delay: null });
      const { input } = renderLateSearch(MergingSearch);

      await user.type(input, "a{Backspace}");
      await act(() => new Promise((resolve) => setTimeout(resolve, 0)));
      await user.click(screen.getByRole("button", { name: "Open a Saved Search for a" }));

      expect(input).toHaveValue("a");
    });
  });

  it("writes a paste run at once into the address the box reads from", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <MemoryRouter initialEntries={["/"]}>
        <AddressSearch onSubmit={onSubmit} />
      </MemoryRouter>,
    );
    const input = screen.getByRole("combobox", { name: "Search messages" });

    await user.click(input);
    await user.paste("attachment:any");
    await user.keyboard("{Enter}");

    expect(onSubmit).toHaveBeenLastCalledWith("attachment:any");
    expect(screen.getByTestId("location").textContent).toBe("?q=attachment%3Aany");
    expect(input).toHaveValue("attachment:any");
  });
});

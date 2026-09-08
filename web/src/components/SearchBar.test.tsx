/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import SearchBar from "./SearchBar";

vi.mock("../lib/recentSearches", () => ({
  loadRecentSearches: () => ["ada", "grace"],
  pushRecentSearch: vi.fn(),
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

describe("SearchBar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    suggestionsMock.current = [];
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
    expect(input.getAttribute("aria-activedescendant")).toBe(first.id);
    expect(first.getAttribute("aria-selected")).toBe("true");

    await user.keyboard("{ArrowDown}");
    const second = screen.getAllByRole("option")[1];
    expect(input.getAttribute("aria-activedescendant")).toBe(second.id);
  });

  it("submits the highlighted recent search on Enter", async () => {
    const user = userEvent.setup();
    const { input, onSubmit } = renderSearch();

    await user.click(input);
    await user.keyboard("{ArrowDown}{Enter}");

    expect(onSubmit).toHaveBeenCalledWith("ada", "contacts");
  });

  it("submits the typed text when no row is highlighted", async () => {
    const user = userEvent.setup();
    const { input, onSubmit } = renderSearch({ value: "typed" });

    await user.click(input);
    await user.keyboard("{Enter}");

    expect(onSubmit).toHaveBeenCalledWith("typed", "contacts");
  });

  it("reaches the advanced-search row by keyboard", async () => {
    const user = userEvent.setup();
    const { input } = renderSearch();

    await user.click(input);
    // Two recents, then the advanced row.
    await user.keyboard("{ArrowDown}{ArrowDown}{ArrowDown}{Enter}");

    expect(await screen.findByTestId("advanced-form")).toBeTruthy();
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
    suggestionsMock.current = [{ id: "handle:", label: "handle:", insert: "handle: " }];
    const user = userEvent.setup();
    const { input } = renderSearch({
      value: "han",
      scope: "message",
      placeholder: "Search messages",
      advancedMode: "messages",
    });

    await user.click(input);
    const labels = screen.getAllByRole("option").map((el) => el.textContent);
    expect(labels).toEqual(["handle:"]);
  });

  it("inserts a suggestion into the query without running the search", async () => {
    suggestionsMock.current = [{ id: "handle:", label: "handle:", insert: "handle: " }];
    const user = userEvent.setup();
    const { input, onChange, onSubmit } = renderSearch({
      value: "han",
      scope: "message",
      placeholder: "Search messages",
      advancedMode: "messages",
    });

    await user.click(input);
    await user.keyboard("{ArrowDown}{Enter}");

    expect(onChange).toHaveBeenCalledWith("handle: ");
    expect(onSubmit).not.toHaveBeenCalled();
  });
});

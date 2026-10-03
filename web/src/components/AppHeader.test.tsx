/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import AppHeader from "./AppHeader";

vi.mock("./AppAccountMenu", () => ({ default: () => null }));
vi.mock("./VersionNotice", () => ({ default: () => null }));
vi.mock("../lib/useSearchSuggestions", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/useSearchSuggestions")>()),
  useSearchSuggestions: () => [],
}));

afterEach(cleanup);

describe("AppHeader", () => {
  it("starts the search box again when a full-screen screen gives way to a list", async () => {
    // Import, Export and Settings have no list, so the box's value there is
    // always "" and what is typed there searches no list. Back on the
    // conversations list, the box shows that list's search, not the leftover
    // text.
    const user = userEvent.setup();
    const props = {
      searchQuery: "",
      searchTarget: "conversations" as const,
      onSearchChange: vi.fn(),
      onSearch: vi.fn(),
    };
    const { rerender } = render(<AppHeader {...props} fullScreen />);
    await user.type(screen.getByRole("combobox", { name: "Search conversations" }), "foo");

    rerender(<AppHeader {...props} fullScreen={false} />);

    expect(screen.getByRole("combobox", { name: "Search conversations" })).toHaveValue("");
  });
});

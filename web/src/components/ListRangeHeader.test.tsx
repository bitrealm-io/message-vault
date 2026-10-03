/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import ListRangeHeader from "./ListRangeHeader";

afterEach(() => {
  cleanup();
});

describe("ListRangeHeader", () => {
  it("shows the range label", () => {
    render(<ListRangeHeader rangeLabel="1–20 of 100" />);
    expect(screen.getByText("1–20 of 100")).toBeInTheDocument();
  });

  it("appends updating and loading suffixes", () => {
    const { rerender } = render(<ListRangeHeader rangeLabel="1–20 of 100" refreshing />);
    expect(screen.getByText(/updating…/)).toBeInTheDocument();

    rerender(<ListRangeHeader rangeLabel="1–20 of 100" filling />);
    expect(screen.getByText(/loading more…/)).toBeInTheDocument();
  });

  it("toggles select-all from the header checkbox", async () => {
    const onSelectAllChange = vi.fn();
    const user = userEvent.setup();
    render(
      <ListRangeHeader
        rangeLabel="1–20 of 100"
        selectAll={{ onChange: onSelectAllChange, label: "Select all contacts" }}
      />,
    );
    await user.click(screen.getByRole("checkbox", { name: "Select all contacts" }));
    expect(onSelectAllChange).toHaveBeenCalledWith(true);
  });

  it("keeps select-all without a range label", async () => {
    const onSelectAllChange = vi.fn();
    const user = userEvent.setup();
    render(
      <ListRangeHeader
        selectAll={{ onChange: onSelectAllChange, label: "Select all contacts" }}
        actions={<button type="button">Sort</button>}
      />,
    );
    expect(screen.queryByText("1–20 of 100")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sort" })).toBeInTheDocument();
    await user.click(screen.getByRole("checkbox", { name: "Select all contacts" }));
    expect(onSelectAllChange).toHaveBeenCalledWith(true);
  });
});

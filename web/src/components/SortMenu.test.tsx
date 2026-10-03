/** @vitest-environment jsdom */

import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import SortMenu from "./SortMenu";

afterEach(cleanup);

describe("SortMenu", () => {
  it("closes its menu when the sort button is clicked again", async () => {
    const user = userEvent.setup();
    render(
      <SortMenu
        fields={[{ id: "name", label: "Name" }]}
        sort="name"
        order="asc"
        onChange={() => {}}
        itemNoun="contacts"
      />,
    );
    const button = screen.getByRole("button", { name: /Sort contacts by/ });
    await user.click(button);
    expect(screen.getByRole("menu")).toBeInTheDocument();

    await user.click(button);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("offers no order for a field that has none, and names none", async () => {
    const user = userEvent.setup();
    render(
      <SortMenu
        fields={[
          { id: "relevance", label: "Relevance" },
          { id: "date", label: "Date" },
        ]}
        unordered={["relevance"]}
        sort="relevance"
        order="desc"
        onChange={() => {}}
        itemNoun="messages"
      />,
    );
    await user.click(screen.getByRole("button", { name: "Sort messages by Relevance" }));
    expect(screen.getAllByRole("menuitemradio").map((item) => item.textContent)).toEqual([
      "Relevance",
      "Date",
    ]);
  });

  it("opens from the keyboard and picks an order by its first letter", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <SortMenu
        fields={[{ id: "name", label: "Name" }]}
        sort="name"
        order="asc"
        onChange={onChange}
        itemNoun="contacts"
      />,
    );
    act(() => screen.getByRole("button", { name: /Sort contacts by/ }).focus());
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("menuitemradio", { name: "Name" })).toHaveFocus();
    expect(screen.getByRole("menuitemradio", { name: "Ascending" })).toHaveAttribute(
      "aria-checked",
      "true",
    );

    await user.keyboard("d");
    expect(screen.getByRole("menuitemradio", { name: "Descending" })).toHaveFocus();
    await user.keyboard("{Enter}");

    expect(onChange).toHaveBeenCalledWith({ sort: "name", order: "desc" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });
});

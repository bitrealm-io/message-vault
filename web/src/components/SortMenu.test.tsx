/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
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
});

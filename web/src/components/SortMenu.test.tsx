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
});

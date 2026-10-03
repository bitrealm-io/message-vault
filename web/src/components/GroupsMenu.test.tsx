/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import GroupsMenu from "./GroupsMenu";

afterEach(() => {
  cleanup();
});

const GROUPS = ["College", "Family", "Work"] as const;
const ROW_TOKENS = ["px-3", "py-1.5", "text-[0.813rem]", "leading-5"] as const;

function renderMenu(labeled = true) {
  return render(
    <GroupsMenu
      allGroups={[...GROUPS]}
      checks={{ College: "on", Family: "off", Work: "off" }}
      labeled={labeled}
      ariaLabel={labeled ? "Contact Groups" : "Message Tags"}
      title={labeled ? "Contact Groups" : "Message Tags"}
      searchPlaceholder={labeled ? "Search Contact Groups…" : "Search Message Tags…"}
      emptyText={labeled ? "No Contact Groups" : "No Message Tags"}
      noMatchText={labeled ? "No matching Contact Groups" : "No matching Message Tags"}
    />,
  );
}

describe("GroupsMenu", () => {
  it("filters labeled group names as the user types", async () => {
    const user = userEvent.setup();
    renderMenu();

    await user.click(screen.getByRole("button", { name: "Contact Groups" }));
    expect(screen.getByText("College")).toBeTruthy();
    expect(screen.getByText("Family")).toBeTruthy();
    expect(screen.getByText("Work")).toBeTruthy();

    await user.type(screen.getByRole("searchbox", { name: "Search Contact Groups…" }), "fam");
    expect(screen.queryByText("College")).toBeNull();
    expect(screen.getByText("Family")).toBeTruthy();
    expect(screen.queryByText("Work")).toBeNull();
  });

  it("keeps the empty message on the same row metrics as a group row", async () => {
    const user = userEvent.setup();
    renderMenu();

    await user.click(screen.getByRole("button", { name: "Contact Groups" }));
    const groupRow = screen.getByText("Family").closest("label");
    expect(groupRow).toBeTruthy();
    for (const token of ROW_TOKENS) {
      expect(groupRow?.className).toContain(token);
    }

    await user.type(screen.getByRole("searchbox", { name: "Search Contact Groups…" }), "zzz");
    const empty = screen.getByRole("status");
    expect(empty.tagName).toBe("DIV");
    expect(empty.textContent).toContain("No matching Contact Groups");
    for (const token of ROW_TOKENS) {
      expect(empty.className).toContain(token);
    }
    expect(screen.queryByText("No Contact Groups")).toBeNull();
  });

  it("shows no groups on the same row when the catalog is empty", async () => {
    const user = userEvent.setup();
    render(
      <GroupsMenu
        allGroups={[]}
        checks={{}}
        labeled
        ariaLabel="Contact Groups"
        title="Contact Groups"
      />,
    );

    await user.click(screen.getByRole("button", { name: "Contact Groups" }));
    const empty = screen.getByRole("status");
    expect(empty.textContent).toContain("No Contact Groups");
    for (const token of ROW_TOKENS) {
      expect(empty.className).toContain(token);
    }
    expect(screen.queryByText("No matching Contact Groups")).toBeNull();
  });

  it("filters the icon-only tags menu", async () => {
    const user = userEvent.setup();
    renderMenu(false);

    await user.click(screen.getByRole("button", { name: "Message Tags" }));
    await user.type(screen.getByRole("searchbox", { name: "Search Message Tags…" }), "wor");
    expect(screen.getByText("Work")).toBeTruthy();
    expect(screen.queryByText("College")).toBeNull();
    expect(screen.queryByText("Family")).toBeNull();
  });
});

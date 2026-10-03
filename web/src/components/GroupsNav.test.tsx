/** @vitest-environment jsdom */

import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { mockedAuth, Providers } from "../test/providers";
import GroupsNav from "./GroupsNav";

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));

afterEach(() => {
  cleanup();
});

function renderNav(path: string) {
  return render(
    <Providers>
      <MemoryRouter initialEntries={[path]}>
        <GroupsNav groups={["College"]} />
      </MemoryRouter>
    </Providers>,
  );
}

function classTokens(el: Element): string[] {
  return el.className.split(/\s+/).filter(Boolean);
}

describe("GroupsNav", () => {
  it("keeps the No group active fill from navGlyphRowClass", () => {
    renderNav("/no-group");
    const btn = screen.getByRole("button", { name: "No Contact Group" });
    const tokens = classTokens(btn);
    expect(tokens).toContain("bg-hover");
    expect(tokens).toContain("font-semibold");
    expect(tokens).toContain("px-0");
    expect(tokens).not.toContain("bg-transparent");
  });

  it("puts group and No group icons in the shared 15px leading slot", () => {
    renderNav("/contacts");
    const college = screen.getByRole("button", { name: "College" });
    const noGroup = screen.getByRole("button", { name: "No Contact Group" });
    expect(college.querySelector('[class*="size-[15px]"]')).not.toBeNull();
    expect(noGroup.querySelector('[class*="size-[15px]"]')).not.toBeNull();
  });

  it("indents nested rows so icons line up with the heading title", () => {
    renderNav("/contacts");
    const college = screen.getByRole("button", { name: "College" });
    expect(college.className).toContain("pl-[calc(15px+0.5rem)]");
    expect(college.className).toContain("self-stretch");
    const noGroupInner = screen
      .getByRole("button", { name: "No Contact Group" })
      .querySelector('[class*="pl-[calc(15px+0.5rem)]"]');
    expect(noGroupInner).not.toBeNull();
    expect(noGroupInner?.className).toContain("self-stretch");
  });

  it("closes the group options menu on Escape", async () => {
    const user = userEvent.setup();
    renderNav("/contacts");
    await user.click(screen.getByRole("button", { name: "Contact Group options for College" }));
    expect(screen.getByRole("menuitem", { name: "Rename…" })).toBeTruthy();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menuitem", { name: "Rename…" })).toBeNull();
  });

  it("exposes the options popup as a menu, not a bare div of buttons", async () => {
    const user = userEvent.setup();
    renderNav("/contacts");
    await user.click(screen.getByRole("button", { name: "Contact Group options for College" }));
    expect(screen.getByRole("menu", { name: "Contact Group options for College" })).toBeTruthy();
    expect(screen.getAllByRole("menuitem")).toHaveLength(2);
  });

  it("moves focus into the menu and walks it with arrow keys", async () => {
    const user = userEvent.setup();
    renderNav("/contacts");
    act(() => screen.getByRole("button", { name: "Contact Group options for College" }).focus());
    await user.keyboard("{Enter}");

    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "Rename…" }));

    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "Delete" }));

    // Wraps around to the first item.
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "Rename…" }));

    await user.keyboard("{ArrowUp}");
    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "Delete" }));
  });

  it("walks a menu opened with the pointer from its first item", async () => {
    const user = userEvent.setup();
    renderNav("/contacts");
    await user.click(screen.getByRole("button", { name: "Contact Group options for College" }));

    // Opened with the pointer, the menu itself has focus, and the first arrow
    // press lands on the first item.
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "Rename…" }));
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "Delete" }));
  });

  it("returns focus to the trigger when the menu closes", async () => {
    const user = userEvent.setup();
    renderNav("/contacts");
    const trigger = screen.getByRole("button", { name: "Contact Group options for College" });
    await user.click(trigger);
    await user.keyboard("{Escape}");
    // React Aria puts focus back a frame after the menu unmounts.
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });
});

/** @vitest-environment jsdom */

/**
 * `rename` and `remove` on `NavEntityList` navigate away from the renamed or
 * deleted item's own page — to the new slug, or to the collection's fallback
 * route. The vault's write routes are faked by name, the way
 * `nameCollection.test.tsx` fakes them, so this never touches a URL string
 * except the one under test.
 */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, useLocation } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { mockedAuth, Providers } from "../test/providers";
import GroupsNav from "./GroupsNav";
import MessageTagsNav from "./MessageTagsNav";

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));

const routes = vi.hoisted(() => ({
  listContactGroups: vi.fn(),
  updateContactGroup: vi.fn(),
  deleteContactGroup: vi.fn(),
  listMessageTags: vi.fn(),
  updateMessageTag: vi.fn(),
  deleteMessageTag: vi.fn(),
}));

vi.mock("../lib/serverApi", () => ({
  listContactGroups: routes.listContactGroups,
  createContactGroup: vi.fn(),
  updateContactGroup: routes.updateContactGroup,
  deleteContactGroup: routes.deleteContactGroup,
  updateContactGroupMembers: vi.fn(),
  listMessageTags: routes.listMessageTags,
  createMessageTag: vi.fn(),
  updateMessageTag: routes.updateMessageTag,
  deleteMessageTag: routes.deleteMessageTag,
  updateMessageTagMembers: vi.fn(),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

/** Renders the current path so a test can read where `navigate` landed. */
// biome-ignore lint/style/useComponentExportOnlyModules: local test harness only
function LocationDisplay() {
  const location = useLocation();
  return <div data-testid="location">{location.pathname}</div>;
}

function renderGroups(path: string, groups: string[]) {
  routes.listContactGroups.mockResolvedValue({
    items: groups.map((name, i) => ({ id: i + 1, name })),
  });
  return render(
    <Providers>
      <MemoryRouter initialEntries={[path]}>
        <LocationDisplay />
        <GroupsNav groups={groups} />
      </MemoryRouter>
    </Providers>,
  );
}

function renderTags(path: string, tags: string[]) {
  routes.listMessageTags.mockResolvedValue({
    items: tags.map((name, i) => ({ id: i + 1, name })),
  });
  return render(
    <Providers>
      <MemoryRouter initialEntries={[path]}>
        <LocationDisplay />
        <MessageTagsNav tags={tags} />
      </MemoryRouter>
    </Providers>,
  );
}

describe("NavEntityList navigation", () => {
  it("follows a renamed group to its new slug when viewing that group's page", async () => {
    routes.updateContactGroup.mockResolvedValue({ id: 1, name: "Fam" });
    const user = userEvent.setup();
    renderGroups("/group/Family", ["Family"]);

    await user.click(screen.getByRole("button", { name: "Group options for Family" }));
    await user.click(screen.getByRole("menuitem", { name: "Rename…" }));
    const input = screen.getByPlaceholderText("Group name");
    await user.clear(input);
    await user.type(input, "Fam");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByTestId("location")).toHaveTextContent("/group/Fam");
  });

  it("falls back to the group collection's home route after deleting the group being viewed", async () => {
    routes.deleteContactGroup.mockResolvedValue(undefined);
    const user = userEvent.setup();
    renderGroups("/group/Family", ["Family"]);

    await user.click(screen.getByRole("button", { name: "Group options for Family" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));

    expect(await screen.findByTestId("location")).toHaveTextContent("/contacts");
  });

  it("follows a renamed tag to its new slug when viewing that tag's page", async () => {
    routes.updateMessageTag.mockResolvedValue({ id: 1, name: "Vacation" });
    const user = userEvent.setup();
    renderTags("/tag/Holiday", ["Holiday"]);

    await user.click(screen.getByRole("button", { name: "Tag options for Holiday" }));
    await user.click(screen.getByRole("menuitem", { name: "Rename…" }));
    const input = screen.getByPlaceholderText("Tag name");
    await user.clear(input);
    await user.type(input, "Vacation");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByTestId("location")).toHaveTextContent("/tag/Vacation");
  });

  it("falls back to the tag collection's home route after deleting the tag being viewed", async () => {
    routes.deleteMessageTag.mockResolvedValue(undefined);
    const user = userEvent.setup();
    renderTags("/tag/Holiday", ["Holiday"]);

    await user.click(screen.getByRole("button", { name: "Tag options for Holiday" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));

    expect(await screen.findByTestId("location")).toHaveTextContent("/");
  });
});

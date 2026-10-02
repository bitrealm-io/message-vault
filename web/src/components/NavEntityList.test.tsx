/** @vitest-environment jsdom */

/**
 * Delete on `NavEntityList` asks first, and `rename` and `remove` navigate away from the renamed or
 * deleted item's own page — to the new slug, or to the collection's fallback
 * route. The server's write routes are faked by name, the way
 * `nameCollection.test.tsx` fakes them, so this never touches a URL string
 * except the one under test.
 */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
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

describe("NavEntityList row menu", () => {
  it("closes the row menu when its options button is clicked again", async () => {
    const user = userEvent.setup();
    renderGroups("/contacts", ["Family"]);

    const options = screen.getByRole("button", { name: "Group options for Family" });
    await user.click(options);
    expect(screen.getByRole("menu", { name: "Group options for Family" })).toBeInTheDocument();

    await user.click(options);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });
});

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
    await user.click(screen.getByRole("button", { name: "Delete" }));

    await waitFor(() => expect(screen.getByTestId("location")).toHaveTextContent("/contacts"));
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
    await user.click(screen.getByRole("button", { name: "Delete" }));

    await waitFor(() => expect(screen.getByTestId("location").textContent).toBe("/"));
  });

  it("asks before deleting a group, and says what goes and what stays", async () => {
    const user = userEvent.setup();
    renderGroups("/group/Family", ["Family"]);

    await user.click(screen.getByRole("button", { name: "Group options for Family" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));

    const dialog = screen.getByRole("dialog", { name: "Delete Family?" });
    expect(dialog).toHaveTextContent(
      "Removes the Contact Group Family and takes every contact out of it. The contacts themselves stay in your Message Crate.",
    );
    expect(routes.deleteContactGroup).not.toHaveBeenCalled();
  });

  it("deletes nothing when the confirmation is cancelled", async () => {
    const user = userEvent.setup();
    renderTags("/tag/Holiday", ["Holiday"]);

    await user.click(screen.getByRole("button", { name: "Tag options for Holiday" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(screen.getByRole("dialog", { name: "Delete Holiday?" })).toHaveTextContent(
      "Removes the Message Tag Holiday and takes it off every conversation that carries it. The conversations themselves stay in your Message Crate.",
    );
    await user.click(screen.getByRole("button", { name: "Cancel" }));

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(routes.deleteMessageTag).not.toHaveBeenCalled();
    expect(screen.getByTestId("location")).toHaveTextContent("/tag/Holiday");
  });

  it("deletes the group once the confirmation is confirmed", async () => {
    routes.deleteContactGroup.mockResolvedValue(undefined);
    const user = userEvent.setup();
    renderGroups("/contacts", ["Family"]);

    await user.click(screen.getByRole("button", { name: "Group options for Family" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));

    await waitFor(() => expect(routes.deleteContactGroup).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("keeps the confirmation open with the server's message when the delete is refused", async () => {
    routes.deleteContactGroup.mockRejectedValue(new Error("group not found"));
    const user = userEvent.setup();
    renderGroups("/group/Family", ["Family"]);

    await user.click(screen.getByRole("button", { name: "Group options for Family" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("group not found");
    expect(screen.getByRole("dialog", { name: "Delete Family?" })).toBeTruthy();
    expect(screen.getByTestId("location")).toHaveTextContent("/group/Family");
  });
});

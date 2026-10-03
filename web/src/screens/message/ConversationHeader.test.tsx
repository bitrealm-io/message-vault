/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createContactGroup,
  getAccountProfile,
  listContactGroups,
  trashConversation,
  updateContactGroupMembers,
} from "../../lib/serverApi";
import type { Conversation } from "../../lib/types";
import { mockedAuth, Providers } from "../../test/providers";
import ConversationHeader from "./ConversationHeader";

vi.mock("../../lib/auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("../../lib/serverApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/serverApi")>()),
  trashConversation: vi.fn(),
  getAccountProfile: vi.fn(),
  listContactGroups: vi.fn(),
  createContactGroup: vi.fn(),
  updateContactGroupMembers: vi.fn(),
}));

const trashConversationMock = vi.mocked(trashConversation);
const getAccountProfileMock = vi.mocked(getAccountProfile);
const listContactGroupsMock = vi.mocked(listContactGroups);
const createContactGroupMock = vi.mocked(createContactGroup);
const updateContactGroupMembersMock = vi.mocked(updateContactGroupMembers);

/** The logged-in account: one phone, so the owner can be told apart from the others. */
const PROFILE = {
  account_id: 7,
  username: "me",
  preferred_name: "Me",
  time_zone: "UTC",
  phones: ["+15550100"],
  emails: [],
  is_owner: false,
  disabled: false,
  must_set_up_profile: false,
  has_password: true,
  is_demo: false,
  can_import: true,
  can_export: true,
  can_delete: true,
  message_count: 0,
  storage_bytes: 0,
};

/** A group chat: the owner, two people with contacts, one nobody has a contact for. */
function groupChat(): Conversation {
  return conversation({
    is_group: true,
    label: "Book Club",
    participants: [
      { name: "Me", identity: "+1 (555) 010-0", contact_id: 1 },
      { name: "Ada", identity: "+15550200", contact_id: 2 },
      { name: "Grace", identity: "+15550300", contact_id: 3 },
      { name: "+15550400", identity: "+15550400", contact_id: null },
    ],
  });
}

function conversation(overrides: Partial<Conversation> = {}): Conversation {
  return {
    id: 42,
    participants: [],
    message_count: 3,
    last_message_at: "",
    date_range_start: null,
    date_range_end: null,
    service: "sms",
    is_group: false,
    label: "Chat 42",
    tags: [],
    ...overrides,
  };
}

function renderHeader(
  c: Conversation,
  {
    years = [],
    onJumpToNewest = () => {},
    onJumpToYear = () => {},
  }: {
    years?: number[];
    onJumpToNewest?: () => void;
    onJumpToYear?: (year: number) => void;
  } = {},
) {
  return render(
    <Providers>
      <MemoryRouter initialEntries={["/messages/42"]}>
        <Routes>
          <Route
            path="/messages/:id"
            element={
              <ConversationHeader
                conversation={c}
                displayParticipants={[]}
                years={years}
                findOpen={false}
                onToggleFind={() => {}}
                onJumpToNewest={onJumpToNewest}
                onJumpToYear={onJumpToYear}
                onShowSources={() => {}}
              />
            }
          />
          <Route path="/" element={<div>Conversations list</div>} />
        </Routes>
      </MemoryRouter>
    </Providers>,
  );
}

/**
 * Open the ⋯ menu and choose an item, opening the menu again until the item
 * is there: an item that waits on a fetch appears once it lands.
 */
async function openMenuItem(user: ReturnType<typeof userEvent.setup>, name: string) {
  await waitFor(async () => {
    if (!screen.queryByRole("menu")) {
      await user.click(screen.getByRole("button", { name: "More for this conversation" }));
    }
    const item = screen.queryByRole("menuitem", { name });
    if (!item) {
      await user.keyboard("{Escape}");
      throw new Error(`no ${name} yet`);
    }
  });
  await user.click(screen.getByRole("menuitem", { name }));
}

describe("ConversationHeader", () => {
  beforeEach(() => {
    trashConversationMock.mockReset();
    getAccountProfileMock.mockReset();
    listContactGroupsMock.mockReset();
    createContactGroupMock.mockReset();
    updateContactGroupMembersMock.mockReset();
    getAccountProfileMock.mockResolvedValue(PROFILE);
    listContactGroupsMock.mockResolvedValue({ items: [], total: 0, limit: 40, offset: 0 });
  });

  afterEach(() => {
    cleanup();
  });

  describe("Make a Contact Group", () => {
    it("is offered on a group chat and not on a direct conversation", async () => {
      const user = userEvent.setup();
      renderHeader(conversation());
      await user.click(screen.getByRole("button", { name: "More for this conversation" }));
      expect(screen.queryByRole("menuitem", { name: "Make a Contact Group" })).toBeNull();
      cleanup();

      renderHeader(groupChat());
      // The owner's profile decides who is left out, so the item waits for it.
      await waitFor(() => expect(getAccountProfileMock).toHaveBeenCalled());
      await openMenuItem(user, "Make a Contact Group");
    });

    it("creates the group and adds everyone but the owner and the contact-less", async () => {
      // The server, modelled: once created, the group is in the list the
      // members call looks the id up in.
      let groups: { id: number; name: string }[] = [];
      listContactGroupsMock.mockImplementation(async () => ({
        items: groups,
        total: groups.length,
        limit: 40,
        offset: 0,
      }));
      createContactGroupMock.mockImplementation(async ({ name }) => {
        const set = { id: 9, name };
        groups = [set];
        return set;
      });
      updateContactGroupMembersMock.mockResolvedValue({ added: 2, removed: 0 });
      const user = userEvent.setup();
      renderHeader(groupChat());

      await openMenuItem(user, "Make a Contact Group");
      // The chat's label is offered as the name.
      const input = screen.getByDisplayValue("Book Club");
      await user.clear(input);
      await user.type(input, "Readers");
      await user.click(screen.getByRole("button", { name: "Create" }));

      await waitFor(() => {
        expect(createContactGroupMock).toHaveBeenCalledWith({ name: "Readers" });
      });
      await waitFor(() => {
        expect(updateContactGroupMembersMock).toHaveBeenCalledWith(9, {
          add: [2, 3],
          remove: [],
        });
      });
      expect(await screen.findByText("Added 2 people to Readers.")).toBeTruthy();
    });

    it("adds to an existing group of that name instead of creating a second one", async () => {
      listContactGroupsMock.mockResolvedValue({
        items: [{ id: 4, name: "Readers" }],
        total: 1,
        limit: 40,
        offset: 0,
      });
      updateContactGroupMembersMock.mockResolvedValue({ added: 2, removed: 0 });
      const user = userEvent.setup();
      renderHeader(groupChat());

      await openMenuItem(user, "Make a Contact Group");
      const input = await screen.findByDisplayValue("Book Club");
      await user.clear(input);
      await user.type(input, "readers");
      await user.click(screen.getByRole("button", { name: "Create" }));

      await waitFor(() => {
        expect(updateContactGroupMembersMock).toHaveBeenCalledWith(4, {
          add: [2, 3],
          remove: [],
        });
      });
      expect(createContactGroupMock).not.toHaveBeenCalled();
    });
  });

  it("says in one line who is in it, the service as the conversation list names it, and how many messages", () => {
    renderHeader(groupChat());
    expect(screen.getByRole("heading", { name: "Book Club" })).toBeInTheDocument();
    expect(screen.getByText("4 people")).toBeInTheDocument();
    // `sms` reads "Text Message", the conversation list's word, not the raw service.
    expect(screen.getByText("Text Message")).toBeInTheDocument();
    expect(screen.getByText("3 messages")).toBeInTheDocument();
  });

  it("offers Newest and every year, newest first, under Jump to", async () => {
    const onJumpToNewest = vi.fn();
    const onJumpToYear = vi.fn();
    const user = userEvent.setup();
    renderHeader(conversation(), { years: [2021, 2022, 2023], onJumpToNewest, onJumpToYear });

    await user.click(screen.getByRole("button", { name: "Jump to ▾" }));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Newest",
      "2023",
      "2022",
      "2021",
    ]);
    await user.click(screen.getByRole("menuitem", { name: "2021" }));
    expect(onJumpToYear).toHaveBeenCalledWith(2021);

    await user.click(screen.getByRole("button", { name: "Jump to ▾" }));
    await user.click(screen.getByRole("menuitem", { name: "Newest" }));
    expect(onJumpToNewest).toHaveBeenCalled();
  });

  it("moves the conversation to trash and navigates back to the conversations list", async () => {
    trashConversationMock.mockResolvedValue(undefined);
    const user = userEvent.setup();
    renderHeader(conversation());

    await openMenuItem(user, "Move to trash");

    expect(trashConversationMock).toHaveBeenCalledWith(42, expect.anything());
    await waitFor(() => {
      expect(screen.getByText("Conversations list")).toBeInTheDocument();
    });
  });

  it("shows an error and stays put when trashing fails", async () => {
    trashConversationMock.mockRejectedValue(new Error("Could not move this conversation."));
    const user = userEvent.setup();
    renderHeader(conversation());

    await openMenuItem(user, "Move to trash");

    expect(await screen.findByText("Could not move this conversation.")).toBeInTheDocument();
    expect(screen.queryByText("Conversations list")).not.toBeInTheDocument();
  });
});

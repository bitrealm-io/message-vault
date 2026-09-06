/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Conversation } from "../../lib/types";
import {
  createContactGroup,
  getAccountProfile,
  listContactGroups,
  trashConversation,
  updateContactGroupMembers,
} from "../../lib/vaultApi";
import { mockedAuth, VaultProviders } from "../../test/vaultProviders";
import ConversationHeader from "./ConversationHeader";

vi.mock("../../lib/auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("../../lib/vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/vaultApi")>()),
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

/** The signed-in account: one phone, so the owner can be told apart from the others. */
const PROFILE = {
  account_id: "acct",
  username: "me",
  preferred_name: "Me",
  time_zone: "UTC",
  phones: ["+15550100"],
  emails: [],
  is_owner: false,
  must_change_password: false,
  must_set_up_profile: false,
  is_demo: false,
  can_import: true,
  can_export: true,
  can_delete: true,
};

/** A group chat: the owner, two people with contacts, one nobody has a contact for. */
function groupChat(): Conversation {
  return conversation({
    is_group: true,
    label: "Book Club",
    participants: [
      { name: "Me", handle: "+1 (555) 010-0", contact_id: 1 },
      { name: "Ada", handle: "+15550200", contact_id: 2 },
      { name: "Grace", handle: "+15550300", contact_id: 3 },
      { name: "+15550400", handle: "+15550400", contact_id: null },
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

function renderHeader(c: Conversation) {
  return render(
    <VaultProviders>
      <MemoryRouter initialEntries={["/messages/42"]}>
        <Routes>
          <Route
            path="/messages/:id"
            element={
              <ConversationHeader
                conversation={c}
                displayParticipants={[]}
                participantsOpen={false}
                onToggleParticipants={() => {}}
                sourceLabel="unknown"
                years={[]}
                activeYear={null}
                onSelectAllYears={() => {}}
                onSelectYear={() => {}}
                onShowSources={() => {}}
              />
            }
          />
          <Route path="/" element={<div>Conversations list</div>} />
        </Routes>
      </MemoryRouter>
    </VaultProviders>,
  );
}

describe("ConversationHeader", () => {
  beforeEach(() => {
    trashConversationMock.mockReset();
    getAccountProfileMock.mockReset();
    listContactGroupsMock.mockReset();
    createContactGroupMock.mockReset();
    updateContactGroupMembersMock.mockReset();
    getAccountProfileMock.mockResolvedValue(PROFILE);
    listContactGroupsMock.mockResolvedValue({ items: [] });
  });

  afterEach(() => {
    cleanup();
  });

  describe("Make a Contact Group", () => {
    it("is offered on a group chat and not on a direct conversation", async () => {
      renderHeader(conversation());
      expect(screen.queryByRole("button", { name: "Make a Contact Group" })).toBeNull();
      cleanup();

      renderHeader(groupChat());
      expect(await screen.findByRole("button", { name: "Make a Contact Group" })).toBeTruthy();
    });

    it("creates the group and adds everyone but the owner and the contact-less", async () => {
      // The vault, modelled: once created, the group is in the list the
      // members call looks the id up in.
      let groups: { id: number; name: string }[] = [];
      listContactGroupsMock.mockImplementation(async () => ({ items: groups }));
      createContactGroupMock.mockImplementation(async ({ name }) => {
        const set = { id: 9, name };
        groups = [set];
        return set;
      });
      updateContactGroupMembersMock.mockResolvedValue({ added: 2, removed: 0 });
      const user = userEvent.setup();
      renderHeader(groupChat());

      await user.click(await screen.findByRole("button", { name: "Make a Contact Group" }));
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
      listContactGroupsMock.mockResolvedValue({ items: [{ id: 4, name: "Readers" }] });
      updateContactGroupMembersMock.mockResolvedValue({ added: 2, removed: 0 });
      const user = userEvent.setup();
      renderHeader(groupChat());

      await user.click(await screen.findByRole("button", { name: "Make a Contact Group" }));
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

  it("moves the conversation to trash and navigates back to the conversations list", async () => {
    trashConversationMock.mockResolvedValue(undefined);
    const user = userEvent.setup();
    renderHeader(conversation());

    const button = screen.getByRole("button", { name: "Move to trash" });
    await user.click(button);

    expect(trashConversationMock).toHaveBeenCalledWith(42, expect.anything());
    await waitFor(() => {
      expect(screen.getByText("Conversations list")).toBeInTheDocument();
    });
  });

  it("shows an error and stays put when trashing fails", async () => {
    trashConversationMock.mockRejectedValue(new Error("Could not move this conversation."));
    const user = userEvent.setup();
    renderHeader(conversation());

    await user.click(screen.getByRole("button", { name: "Move to trash" }));

    expect(await screen.findByText("Could not move this conversation.")).toBeInTheDocument();
    expect(screen.queryByText("Conversations list")).not.toBeInTheDocument();
  });
});

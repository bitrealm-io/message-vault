/** @vitest-environment jsdom */

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Conversation } from "../lib/types";
import {
  deleteContact,
  deleteConversation,
  emptyTrash,
  getAccountProfile,
  getConversation,
  listContacts,
  listConversations,
  listSearchFields,
  restoreContact,
  restoreConversation,
} from "../lib/vaultApi";
import { mockedAuth, VaultProviders } from "../test/vaultProviders";
import TrashScreen from "./TrashScreen";

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));

vi.mock("../lib/vaultApi", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/vaultApi")>()),
  listConversations: vi.fn(),
  listContacts: vi.fn(),
  listSearchFields: vi.fn(),
  getConversation: vi.fn(),
  getAccountProfile: vi.fn(),
  restoreConversation: vi.fn(),
  restoreContact: vi.fn(),
  deleteConversation: vi.fn(),
  deleteContact: vi.fn(),
  emptyTrash: vi.fn(),
}));

const listConversationsMock = vi.mocked(listConversations);
const listContactsMock = vi.mocked(listContacts);
const listSearchFieldsMock = vi.mocked(listSearchFields);
const getAccountProfileMock = vi.mocked(getAccountProfile);
const deleteConversationMock = vi.mocked(deleteConversation);
const deleteContactMock = vi.mocked(deleteContact);
const emptyTrashMock = vi.mocked(emptyTrash);

/** The signed-in account's profile, as far as this screen reads it. */
function profile(can_delete: boolean): Awaited<ReturnType<typeof getAccountProfile>> {
  return { can_delete } as unknown as Awaited<ReturnType<typeof getAccountProfile>>;
}

/** The words each list accepts, as `GET /v1/search-fields` would say: enough of
 * the registry (search/fields.rs) to tell a shared word from a one-list word. */
const FIELD_WORDS = {
  contacts: ["name", "handle", "messages", "conversations", "trashed"],
  conversations: ["name", "handle", "messages", "participants", "trashed"],
  messages: ["body", "from", "to", "in", "trashed"],
} as const;

function fieldsFor(list: keyof typeof FIELD_WORDS) {
  return {
    list,
    items: FIELD_WORDS[list].map((word) => ({
      word,
      value_type: "text" as const,
      values: [],
      help: "",
      example: `${word}:x`,
    })),
  };
}
const getConversationMock = vi.mocked(getConversation);
const restoreConversationMock = vi.mocked(restoreConversation);
const restoreContactMock = vi.mocked(restoreContact);

function conversation(overrides: Partial<Conversation> = {}): Conversation {
  return {
    id: 42,
    participants: [{ name: "Ada Lovelace" }],
    message_count: 5,
    last_message_at: "",
    date_range_start: null,
    date_range_end: null,
    service: "sms",
    is_group: false,
    label: null,
    tags: [],
    ...overrides,
  };
}

/** One trashed contact as `GET /v1/contacts?q=trashed:yes` returns it. */
function contact(id: number, name: string) {
  return { id, name, handle_count: 1, last_modified: "2026-09-04T00:00:00Z" };
}

function contactPage(items: ReturnType<typeof contact>[]) {
  return { items, total: items.length, limit: 100, offset: 0 };
}

function renderAt(path: string) {
  return render(
    <VaultProviders>
      <MemoryRouter initialEntries={[path]}>
        <TrashScreen />
      </MemoryRouter>
    </VaultProviders>,
  );
}

describe("TrashScreen", () => {
  beforeEach(() => {
    listConversationsMock.mockReset();
    listContactsMock.mockReset();
    getConversationMock.mockReset();
    restoreConversationMock.mockReset();
    restoreContactMock.mockReset();
    getAccountProfileMock.mockReset();
    deleteConversationMock.mockReset();
    deleteContactMock.mockReset();
    emptyTrashMock.mockReset();
    getAccountProfileMock.mockResolvedValue(profile(true));
    deleteConversationMock.mockResolvedValue(undefined);
    deleteContactMock.mockResolvedValue(undefined);
    emptyTrashMock.mockResolvedValue(undefined);
    listSearchFieldsMock.mockReset();
    listSearchFieldsMock.mockImplementation(async (list) =>
      fieldsFor(list as keyof typeof FIELD_WORDS),
    );
    listConversationsMock.mockResolvedValue({ items: [], total: 1, limit: 1, offset: 0 });
    listContactsMock.mockResolvedValue(contactPage([]));
    getConversationMock.mockResolvedValue(conversation());
    restoreConversationMock.mockResolvedValue(undefined);
    restoreContactMock.mockResolvedValue(undefined);
  });

  afterEach(() => {
    cleanup();
  });

  it("reports the trash count and prompts selection when nothing is selected", async () => {
    renderAt("/trash");

    expect(await screen.findByText(/in Trash\. Select one on the left to view it\./)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Restore" })).toBeNull();
  });

  it("explains a word one list refuses instead of asking that list", async () => {
    // `participants:` is a conversations word. The contacts pane must not
    // send it (the vault would answer 400) and must say who the word is for;
    // the conversations pane still answers normally (#331).
    renderAt("/trash?tq=participants%3A%3E3");

    expect(await screen.findByText("participants: applies to conversations only")).toBeTruthy();
    expect(await screen.findByText(/1 conversation matching this search in Trash/)).toBeTruthy();
    expect(listContactsMock).not.toHaveBeenCalled();
    expect(listConversationsMock).toHaveBeenCalled();
  });

  it("explains a contacts-only word in the conversations pane and lists contacts normally", async () => {
    listContactsMock.mockResolvedValue(contactPage([contact(7, "Ada Lovelace")]));
    renderAt("/trash?tq=conversations%3A0");

    expect(await screen.findByText("conversations: applies to contacts only")).toBeTruthy();
    expect(await screen.findByText("Ada Lovelace")).toBeTruthy();
    expect(listConversationsMock).not.toHaveBeenCalled();
  });

  it("reads correctly when the trash is empty", async () => {
    listConversationsMock.mockResolvedValue({ items: [], total: 0, limit: 1, offset: 0 });
    renderAt("/trash");

    expect(await screen.findByText("Trash is empty.")).toBeTruthy();
    expect(screen.queryByText("Conversations")).toBeNull();
    expect(screen.queryByText("Contacts")).toBeNull();
  });

  it("shows Restore for a selected trashed conversation and calls the restore mutation", async () => {
    const user = userEvent.setup();
    renderAt("/trash?tsel=42");

    expect(await screen.findByText("Ada Lovelace")).toBeTruthy();
    const button = screen.getByRole("button", { name: "Restore" });

    await user.click(button);

    await waitFor(() => {
      expect(restoreConversationMock).toHaveBeenCalledWith(42, expect.anything());
    });
  });

  it("clears the selection once restore succeeds, leaving the row out of the trash view", async () => {
    const user = userEvent.setup();
    renderAt("/trash?tsel=42");

    expect(await screen.findByText("Ada Lovelace")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Restore" }));

    await waitFor(() => {
      expect(screen.queryByText("Ada Lovelace")).toBeNull();
      expect(screen.queryByRole("button", { name: "Restore" })).toBeNull();
    });
    expect(
      screen.getByText(/in Trash\. Select one on the left to view it\.|Trash is empty\./),
    ).toBeTruthy();
  });

  it("shows an error and keeps the selection when restoring fails", async () => {
    restoreConversationMock.mockRejectedValue(new Error("Could not restore this conversation."));
    const user = userEvent.setup();
    renderAt("/trash?tsel=42");

    expect(await screen.findByText("Ada Lovelace")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Restore" }));

    expect(await screen.findByText("Could not restore this conversation.")).toBeTruthy();
    expect(screen.getByText("Ada Lovelace")).toBeTruthy();
  });

  describe("trashed contacts", () => {
    it("lists trashed contacts, asking the vault for them with trashed:yes", async () => {
      listContactsMock.mockResolvedValue(contactPage([contact(7, "Grace Hopper")]));
      renderAt("/trash");

      expect(await screen.findByText("Grace Hopper")).toBeTruthy();
      expect(listContactsMock).toHaveBeenCalledWith(
        expect.objectContaining({ q: "trashed:yes" }),
        expect.anything(),
      );
    });

    it("restores a contact from its row and drops it from the list", async () => {
      const user = userEvent.setup();
      // The vault, modelled: restoring takes the contact out of the trash, so
      // the refetch the mutation triggers answers with an empty page.
      let trashed = [contact(7, "Grace Hopper")];
      listContactsMock.mockImplementation(async () => contactPage(trashed));
      restoreContactMock.mockImplementation(async () => {
        trashed = [];
      });
      renderAt("/trash");

      await user.click(await screen.findByRole("button", { name: "Restore Grace Hopper" }));

      await waitFor(() => {
        expect(restoreContactMock).toHaveBeenCalledWith(7, expect.anything());
      });

      await waitFor(() => {
        expect(screen.queryByText("Grace Hopper")).toBeNull();
      });
      expect(screen.getByText("No contacts in Trash.")).toBeTruthy();
    });

    it("says so when conversations are in the trash but no contacts are", async () => {
      renderAt("/trash");

      expect(await screen.findByText("No contacts in Trash.")).toBeTruthy();
      expect(screen.getByText(/in Trash\. Select one on the left to view it\./)).toBeTruthy();
    });

    it("shows both sections when the trash holds conversations and contacts", async () => {
      listContactsMock.mockResolvedValue(contactPage([contact(7, "Grace Hopper")]));
      renderAt("/trash");

      expect(await screen.findByText("Grace Hopper")).toBeTruthy();
      expect(screen.getByText("Conversations")).toBeTruthy();
      expect(screen.getByText("Contacts")).toBeTruthy();
    });

    it("keeps the contacts section when only contacts are in the trash", async () => {
      listConversationsMock.mockResolvedValue({ items: [], total: 0, limit: 1, offset: 0 });
      listContactsMock.mockResolvedValue(contactPage([contact(7, "Grace Hopper")]));
      renderAt("/trash");

      expect(await screen.findByText("Grace Hopper")).toBeTruthy();
      expect(screen.getByText("No conversations in Trash.")).toBeTruthy();
    });

    it("shows an error when restoring a contact fails", async () => {
      restoreContactMock.mockRejectedValue(new Error("Could not restore this contact."));
      listContactsMock.mockResolvedValue(contactPage([contact(7, "Grace Hopper")]));
      const user = userEvent.setup();
      renderAt("/trash");

      await user.click(await screen.findByRole("button", { name: "Restore Grace Hopper" }));

      expect(await screen.findByText("Could not restore this contact.")).toBeTruthy();
      expect(screen.getByText("Grace Hopper")).toBeTruthy();
    });

    it("narrows both kinds with the header search term", async () => {
      listContactsMock.mockResolvedValue(contactPage([]));
      renderAt("/trash?tq=ada");

      expect(await screen.findByText("No contacts match this search.")).toBeTruthy();
      expect(listContactsMock).toHaveBeenCalledWith(
        expect.objectContaining({ q: "trashed:yes ada" }),
        expect.anything(),
      );
    });
  });

  describe("permanent delete", () => {
    it("deletes the selected conversation after the dialog is confirmed, and clears the selection", async () => {
      const user = userEvent.setup();
      renderAt("/trash?tsel=42");

      expect(await screen.findByText("Ada Lovelace")).toBeTruthy();
      await user.click(screen.getByRole("button", { name: "Delete" }));

      const dialog = await screen.findByRole("dialog", { name: "Delete this conversation?" });
      expect(
        within(dialog).getByText(/Deletes Ada Lovelace and its 5 messages from your vault/),
      ).toBeTruthy();
      // Nothing is sent until the dialog is confirmed.
      expect(deleteConversationMock).not.toHaveBeenCalled();

      await user.click(within(dialog).getByRole("button", { name: "Delete" }));

      await waitFor(() => {
        expect(deleteConversationMock).toHaveBeenCalledWith(42, expect.anything());
      });
      await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
        expect(screen.queryByText("Ada Lovelace")).toBeNull();
      });
    });

    it("keeps the dialog open and shows why when deleting a conversation fails", async () => {
      deleteConversationMock.mockRejectedValue(new Error("Could not delete this conversation."));
      const user = userEvent.setup();
      renderAt("/trash?tsel=42");

      await screen.findByText("Ada Lovelace");
      await user.click(screen.getByRole("button", { name: "Delete" }));
      const dialog = await screen.findByRole("dialog");
      await user.click(within(dialog).getByRole("button", { name: "Delete" }));

      expect(await within(dialog).findByRole("alert")).toHaveTextContent(
        "Could not delete this conversation.",
      );
      expect(screen.getByText("Ada Lovelace")).toBeTruthy();
    });

    it("deletes a contact from its row after a dialog that says the messages stay", async () => {
      const user = userEvent.setup();
      let trashed = [contact(7, "Grace Hopper")];
      listContactsMock.mockImplementation(async () => contactPage(trashed));
      deleteContactMock.mockImplementation(async () => {
        trashed = [];
      });
      renderAt("/trash");

      await user.click(await screen.findByRole("button", { name: "Delete Grace Hopper" }));

      const dialog = await screen.findByRole("dialog", { name: "Delete Grace Hopper?" });
      expect(
        within(dialog).getByText(
          "The name and details go, and the contact becomes Unknown. The messages stay, showing the phone number or address instead.",
        ),
      ).toBeTruthy();
      await user.click(within(dialog).getByRole("button", { name: "Delete" }));

      await waitFor(() => {
        expect(deleteContactMock).toHaveBeenCalledWith(7, expect.anything());
      });
      await waitFor(() => {
        expect(screen.queryByText("Grace Hopper")).toBeNull();
      });
      expect(screen.getByText("No contacts in Trash.")).toBeTruthy();
    });

    it("empties the trash after the dialog is confirmed", async () => {
      const user = userEvent.setup();
      listContactsMock.mockResolvedValue(contactPage([contact(7, "Grace Hopper")]));
      renderAt("/trash");

      await user.click(await screen.findByRole("button", { name: "Empty Trash" }));

      const dialog = await screen.findByRole("dialog", { name: "Empty Trash?" });
      expect(
        within(dialog).getByText(
          /Every contact in Trash loses its name and details and becomes Unknown; their messages stay\./,
        ),
      ).toBeTruthy();
      expect(emptyTrashMock).not.toHaveBeenCalled();

      await user.click(within(dialog).getByRole("button", { name: "Empty Trash" }));

      await waitFor(() => {
        expect(emptyTrashMock).toHaveBeenCalledTimes(1);
      });
      await waitFor(() => {
        expect(screen.queryByRole("dialog")).toBeNull();
      });
    });

    it("says Empty Trash empties all of Trash while a search narrows the view", async () => {
      const user = userEvent.setup();
      renderAt("/trash?tq=ada");

      await user.click(await screen.findByRole("button", { name: "Empty Trash" }));

      const dialog = await screen.findByRole("dialog", { name: "Empty Trash?" });
      expect(
        within(dialog).getByText(/This empties all of Trash, not only what matches the search\./),
      ).toBeTruthy();
    });

    it("does not offer Empty Trash when the trash is empty", async () => {
      listConversationsMock.mockResolvedValue({ items: [], total: 0, limit: 1, offset: 0 });
      renderAt("/trash");

      expect(await screen.findByText("Trash is empty.")).toBeTruthy();
      expect(screen.queryByRole("button", { name: "Empty Trash" })).toBeNull();
    });

    it("disables Delete and Empty Trash for an account that may not delete", async () => {
      getAccountProfileMock.mockResolvedValue(profile(false));
      listContactsMock.mockResolvedValue(contactPage([contact(7, "Grace Hopper")]));
      renderAt("/trash?tsel=42");

      expect(await screen.findByText("Ada Lovelace")).toBeTruthy();
      await waitFor(() => {
        expect(screen.getByRole("button", { name: "Empty Trash" })).toBeDisabled();
      });
      expect(screen.getByRole("button", { name: "Delete" })).toBeDisabled();
      expect(screen.getByRole("button", { name: "Delete Grace Hopper" })).toBeDisabled();
      // Restore stays available: only deleting needs the grant.
      expect(screen.getByRole("button", { name: "Restore" })).not.toBeDisabled();
      expect(screen.getByRole("button", { name: "Restore Grace Hopper" })).not.toBeDisabled();
    });
  });
});

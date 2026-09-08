/** @vitest-environment jsdom */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render as rtlRender, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ContactDetail } from "../lib/contactDetail";
import { keys } from "../lib/vaultKeys";
import { vaultQueryKey } from "../lib/vaultQueryKey";
import ContactDrawer from "./ContactDrawer";

vi.mock("../lib/auth", () => ({ useAuth: () => ({ accountId: 7 }) }));

let client: QueryClient;

/** Render inside a fresh cache, the way the app renders inside the app's. */
function render(ui: ReactElement) {
  return rtlRender(ui, {
    wrapper: ({ children }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  });
}

/** Put a contact in the cache, as an earlier open of the drawer would have. */
function seed(detail: ContactDetail): void {
  client.setQueryData(vaultQueryKey(7, keys.contacts.detail(detail.id)), detail);
}

const get = vi.fn();
const post = vi.fn();

const trash = vi.fn();

vi.mock("../lib/vaultApi", () => ({
  getContact: (...args: unknown[]) => get(...args),
  updateContact: (...args: unknown[]) => post(...args),
  trashContact: (...args: unknown[]) => trash(...args),
}));
function detail(id: number, overrides: Partial<ContactDetail> = {}): ContactDetail {
  return {
    id,
    name: `Contact ${id}`,
    last_modified: "2024-01-01T00:00:00Z",
    handles: [
      {
        handle: `+1555000${id}`,
        service: "phone",
        start_date: "2020-01-01T00:00:00Z",
        end_date: "2024-01-01T00:00:00Z",
        individual_conversations: 3,
        group_conversations: 1,
        individual_message_count: 42,
        group_message_count: 7,
      },
    ],
    direct_conversations: 3,
    group_conversations: 1,
    total_messages: 49,
    groups: [`Group-${id}`],
    ...overrides,
  };
}

describe("ContactDrawer", () => {
  afterEach(() => {
    cleanup();
  });

  beforeEach(() => {
    get.mockReset();
    post.mockReset();
    post.mockResolvedValue(undefined);
    trash.mockReset();
    trash.mockResolvedValue(undefined);
    client = new QueryClient({
      // Seeded entries have no observer until the drawer opens that contact, so
      // they must survive collection to stand in for an earlier open.
      defaultOptions: {
        queries: {
          retry: false,
          gcTime: Number.POSITIVE_INFINITY,
          staleTime: Number.POSITIVE_INFINITY,
        },
      },
    });
  });

  it("keeps groups and avoids zero counts on first paint when switching to an uncached contact", async () => {
    const a = detail(1);
    seed(a);

    let resolveB!: (d: ContactDetail) => void;
    const pendingB = new Promise<ContactDetail>((resolve) => {
      resolveB = resolve;
    });
    get.mockImplementation((id: string) => {
      if (String(id) === "1") return Promise.resolve(a);
      return pendingB;
    });

    const { rerender } = render(
      <ContactDrawer
        variant="docked"
        contactId="1"
        preview={{
          id: "1",
          name: a.name,
          handles: a.handles.map((h) => h.handle),
          groups: a.groups,
        }}
        onClose={() => {}}
      />,
    );

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: a.name })).toBeTruthy();
      expect(screen.getByText("Group-1")).toBeTruthy();
    });

    rerender(
      <ContactDrawer
        variant="docked"
        contactId="2"
        preview={{
          id: "2",
          name: "Contact b",
          handles: ["+1555000b"],
          groups: ["Family"],
        }}
        onClose={() => {}}
      />,
    );

    const dialog = screen.getByRole("dialog", { name: "Contact b" });
    expect(dialog.getAttribute("aria-busy")).toBe("true");
    expect(screen.getByText("Family")).toBeTruthy();
    expect(screen.getByText("+1555000b")).toBeTruthy();

    const table = screen.getByRole("grid", { name: "Contact handles" });
    const dashes = table.textContent?.match(/—/g) ?? [];
    expect(dashes.length).toBeGreaterThanOrEqual(4);

    resolveB(detail(2, { name: "Contact b", groups: ["Family"] }));
    await waitFor(() => {
      expect(dialog.getAttribute("aria-busy")).toBeNull();
      expect(screen.getAllByText("42").length).toBeGreaterThan(0);
    });
  });

  it("shows cached counts and groups on first paint when switching to a cached contact", async () => {
    const a = detail(1);
    const b = detail(2, {
      name: "Cached Bob",
      groups: ["Work"],
      handles: [
        {
          handle: "+15551212",
          service: "phone",
          start_date: "2021-06-01T00:00:00Z",
          end_date: "2025-01-01T00:00:00Z",
          individual_conversations: 5,
          group_conversations: 2,
          individual_message_count: 99,
          group_message_count: 11,
        },
      ],
    });
    seed(a);
    seed(b);

    get.mockImplementation((id: string) => {
      if (String(id) === "1") return Promise.resolve(a);
      return Promise.resolve(b);
    });

    const { rerender } = render(
      <ContactDrawer
        variant="docked"
        contactId="1"
        preview={{ id: "1", name: a.name, handles: ["+1555000a"], groups: a.groups }}
        onClose={() => {}}
      />,
    );

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: a.name })).toBeTruthy();
    });

    rerender(
      <ContactDrawer
        variant="docked"
        contactId="2"
        preview={{
          id: "2",
          name: "Cached Bob",
          handles: ["+15551212"],
          groups: ["Work"],
        }}
        onClose={() => {}}
      />,
    );

    expect(screen.getByRole("heading", { name: "Cached Bob" })).toBeTruthy();
    expect(screen.getByRole("dialog").getAttribute("aria-busy")).toBeNull();
    expect(screen.getByText("Work")).toBeTruthy();
    expect(screen.getAllByText("99").length).toBeGreaterThan(0);
    expect(screen.getAllByText("11").length).toBeGreaterThan(0);
  });

  it("does not claim No groups while loading without preview groups", async () => {
    let resolveDetail!: (d: ContactDetail) => void;
    const pending = new Promise<ContactDetail>((resolve) => {
      resolveDetail = resolve;
    });
    get.mockImplementation(() => pending);

    render(<ContactDrawer variant="overlay" contactId="26" preview={null} onClose={() => {}} />);

    const dialog = screen.getByRole("dialog", { name: "Loading…" });
    expect(dialog.getAttribute("aria-busy")).toBe("true");
    expect(screen.queryByText("No groups")).toBeNull();
    expect(screen.getByText("…")).toBeTruthy();

    resolveDetail(detail(26, { name: "Zed", groups: ["Work"] }));
    await waitFor(() => {
      expect(screen.getByRole("dialog", { name: "Zed" })).toBeTruthy();
      expect(screen.getByText("Work")).toBeTruthy();
      expect(screen.queryByText("No groups")).toBeNull();
    });
  });

  it("stubs one handle row when preview lists raw and normalized forms of the same identity", async () => {
    let resolveDetail!: (d: ContactDetail) => void;
    const pending = new Promise<ContactDetail>((resolve) => {
      resolveDetail = resolve;
    });
    get.mockImplementation(() => pending);

    render(
      <ContactDrawer
        variant="docked"
        contactId="2"
        preview={{
          id: "2",
          name: "Contact b",
          handles: ["+1555000b", "1555000b"],
          handleCount: 1,
          groups: ["Family"],
        }}
        onClose={() => {}}
      />,
    );

    const dialog = screen.getByRole("dialog", { name: "Contact b" });
    expect(dialog.getAttribute("aria-busy")).toBe("true");
    expect(screen.getByText("+1555000b")).toBeTruthy();
    expect(screen.queryByText("1555000b")).toBeNull();

    const table = screen.getByRole("grid", { name: "Contact handles" });
    // Header + one handle row + summary row.
    expect(table.querySelectorAll('[role="row"]').length).toBe(3);

    resolveDetail(
      detail(2, {
        name: "Contact b",
        groups: ["Family"],
        handles: [
          {
            handle: "+1555000b",
            service: "phone",
            start_date: "2020-01-01T00:00:00Z",
            end_date: "2024-01-01T00:00:00Z",
            individual_conversations: 3,
            group_conversations: 1,
            individual_message_count: 42,
            group_message_count: 7,
          },
        ],
      }),
    );
    await waitFor(() => {
      expect(dialog.getAttribute("aria-busy")).toBeNull();
      expect(table.querySelectorAll('[role="row"]').length).toBe(3);
    });
  });

  it("stubs overlay handles from thread preview while detail is pending", async () => {
    let resolveDetail!: (d: ContactDetail) => void;
    const pending = new Promise<ContactDetail>((resolve) => {
      resolveDetail = resolve;
    });
    get.mockImplementation(() => pending);

    render(
      <ContactDrawer
        variant="overlay"
        contactId="2"
        preview={{
          id: "2",
          name: "Contact b",
          handles: ["+1555000b"],
          handleCount: 1,
        }}
        onClose={() => {}}
      />,
    );

    expect(screen.getByRole("heading", { name: "Contact b" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "Loading…" })).toBeNull();
    expect(screen.getByText("+1555000b")).toBeTruthy();

    const table = screen.getByRole("grid", { name: "Contact handles" });
    expect(table.querySelectorAll('[role="row"]').length).toBe(3);

    resolveDetail(
      detail(2, {
        name: "Contact b",
        handles: [
          {
            handle: "+1555000b",
            service: "phone",
            start_date: "2020-01-01T00:00:00Z",
            end_date: "2024-01-01T00:00:00Z",
            individual_conversations: 3,
            group_conversations: 1,
            individual_message_count: 42,
            group_message_count: 7,
          },
        ],
      }),
    );
    await waitFor(() => {
      expect(table.querySelectorAll('[role="row"]').length).toBe(3);
    });
  });

  it("stubs one overlay identity row when thread preview has no handle strings", async () => {
    let resolveDetail!: (d: ContactDetail) => void;
    const pending = new Promise<ContactDetail>((resolve) => {
      resolveDetail = resolve;
    });
    get.mockImplementation(() => pending);

    render(
      <ContactDrawer
        variant="overlay"
        contactId="2"
        preview={{
          id: "2",
          name: "Mom",
          handles: [],
          handleCount: 1,
        }}
        onClose={() => {}}
      />,
    );

    expect(screen.getByRole("heading", { name: "Mom" })).toBeTruthy();
    expect(screen.queryByText("Loading…")).toBeNull();

    const table = screen.getByRole("grid", { name: "Contact handles" });
    expect(table.querySelectorAll('[role="row"]').length).toBe(3);
    expect(table.textContent).toContain("…");

    resolveDetail(detail(2, { name: "Ada Lovelace" }));
    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Ada Lovelace" })).toBeTruthy();
    });
  });

  it("stubs two identities when preview lists raw then normalized for each", async () => {
    let resolveDetail!: (d: ContactDetail) => void;
    const pending = new Promise<ContactDetail>((resolve) => {
      resolveDetail = resolve;
    });
    get.mockImplementation(() => pending);

    render(
      <ContactDrawer
        variant="docked"
        contactId="2"
        preview={{
          id: "2",
          name: "Contact b",
          handles: ["+15550001", "15550001", "+15550002", "15550002"],
          handleCount: 2,
          groups: ["Family"],
        }}
        onClose={() => {}}
      />,
    );

    expect(screen.getByText("+15550001")).toBeTruthy();
    expect(screen.getByText("+15550002")).toBeTruthy();
    expect(screen.queryByText("15550001")).toBeNull();
    expect(screen.queryByText("15550002")).toBeNull();
    const table = screen.getByRole("grid", { name: "Contact handles" });
    expect(table.querySelectorAll('[role="row"]').length).toBe(4);

    resolveDetail(
      detail(2, {
        name: "Contact b",
        groups: ["Family"],
        handles: [
          {
            handle: "+15550001",
            service: "phone",
            start_date: "2020-01-01T00:00:00Z",
            end_date: "2024-01-01T00:00:00Z",
            individual_conversations: 1,
            group_conversations: 0,
            individual_message_count: 4,
            group_message_count: 0,
          },
          {
            handle: "+15550002",
            service: "phone",
            start_date: "2020-01-01T00:00:00Z",
            end_date: "2024-01-01T00:00:00Z",
            individual_conversations: 2,
            group_conversations: 0,
            individual_message_count: 8,
            group_message_count: 0,
          },
        ],
      }),
    );
    await waitFor(() => {
      expect(table.querySelectorAll('[role="row"]').length).toBe(4);
    });
  });

  it("keeps the edit-name control mounted and disabled while detail is loading", async () => {
    let resolveDetail!: (d: ContactDetail) => void;
    const pending = new Promise<ContactDetail>((resolve) => {
      resolveDetail = resolve;
    });
    get.mockImplementation(() => pending);

    render(
      <ContactDrawer
        variant="docked"
        contactId="2"
        preview={{
          id: "2",
          name: "Contact b",
          handles: ["+1555000b"],
          handleCount: 1,
          groups: ["Family"],
        }}
        onClose={() => {}}
      />,
    );

    const edit = screen.getByRole("button", { name: "Edit name" });
    expect(edit).toBeDisabled();

    resolveDetail(detail(2, { name: "Contact b", groups: ["Family"] }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Edit name" })).not.toBeDisabled();
    });
  });

  async function openNameEditor(user: ReturnType<typeof userEvent.setup>) {
    get.mockResolvedValue(detail(1, { name: "Contact a" }));
    render(
      <ContactDrawer
        variant="docked"
        contactId="1"
        preview={{
          id: "1",
          name: "Contact a",
          handles: ["+1555000a"],
          groups: [],
        }}
        onClose={() => {}}
      />,
    );
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Edit name" })).not.toBeDisabled();
    });
    await user.click(screen.getByRole("button", { name: "Edit name" }));
    return screen.getByRole("textbox", { name: "Contact name" });
  }

  it("constrains the name editor to at most half of the title slot", async () => {
    const user = userEvent.setup();
    const input = await openNameEditor(user);
    const wrapper = input.parentElement;
    expect(wrapper?.className).toMatch(/max-w-\[50%\]/);
    expect(wrapper?.className).toMatch(/min-w-\[8rem\]/);
    expect(input.className).toMatch(/\bh-7\b/);
    expect(input.className).toMatch(/w-full/);
    expect(wrapper?.className).not.toMatch(/\bw-full\b/);
    expect(wrapper?.className).not.toMatch(/w-1\/2/);
  });

  it("cancels name edit on Escape without saving", async () => {
    const user = userEvent.setup();
    const input = await openNameEditor(user);
    await user.clear(input);
    await user.type(input, "Renamed");
    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Contact a" })).toBeTruthy();
    });
    expect(post).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Edit name" })).toBeTruthy();
  });

  it("cancels name edit on blur without saving", async () => {
    const user = userEvent.setup();
    const input = await openNameEditor(user);
    await user.clear(input);
    await user.type(input, "Renamed");
    await user.tab();
    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Contact a" })).toBeTruthy();
    });
    expect(post).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Edit name" })).toBeTruthy();
  });

  it("cancels name edit when clicking Contact groups without saving", async () => {
    const user = userEvent.setup();
    const input = await openNameEditor(user);
    await user.clear(input);
    await user.type(input, "Renamed");
    await user.click(screen.getByText("Contact groups"));
    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Contact a" })).toBeTruthy();
    });
    expect(post).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Edit name" })).toBeTruthy();
  });

  it("saves the name on Enter even if the field blurs", async () => {
    const user = userEvent.setup();
    const input = await openNameEditor(user);
    await user.clear(input);
    await user.type(input, "Renamed");
    await user.keyboard("{Enter}");
    input.blur();
    await waitFor(() => {
      expect(post).toHaveBeenCalledWith("1", { name: "Renamed" });
    });
  });

  it("centers identity headers between column markers and keeps Group last", async () => {
    get.mockResolvedValue(detail(1));
    render(
      <ContactDrawer
        variant="docked"
        contactId="1"
        preview={{
          id: "1",
          name: "Contact a",
          handles: ["+1555000a"],
          groups: [],
        }}
        onClose={() => {}}
      />,
    );

    await waitFor(() => {
      expect(screen.getByRole("grid", { name: "Contact handles" })).toBeTruthy();
    });

    const service = screen.getByRole("columnheader", { name: /Service/i });
    expect(service.className).toMatch(/text-center/);
    expect(service.className).not.toMatch(/text-left/);

    const firstSeen = screen.getByRole("columnheader", { name: /First Seen/i });
    expect(firstSeen.querySelector(".whitespace-nowrap")).toBeTruthy();
    expect(firstSeen.className).toMatch(/text-center/);
    expect(firstSeen.className).not.toMatch(/text-left/);
    const lastSeen = screen.getByRole("columnheader", { name: /Last Seen/i });
    expect(lastSeen.querySelector(".whitespace-nowrap")).toBeTruthy();
    expect(lastSeen.className).toMatch(/text-center/);
    expect(lastSeen.className).not.toMatch(/text-left/);

    const threads = screen.getByRole("columnheader", { name: /Threads/i });
    expect(threads.className).toMatch(/text-center/);
    expect(threads.className).not.toMatch(/text-right/);

    const direct = screen.getByRole("columnheader", { name: /Direct Messages/i });
    expect(direct.querySelector(".flex-col")).toBeTruthy();
    expect(direct.querySelector(".items-center")).toBeTruthy();
    expect(direct.className).toMatch(/text-center/);
    expect(direct.className).not.toMatch(/text-right/);
    const group = screen.getByRole("columnheader", { name: /Group Messages/i });
    expect(group.querySelector(".flex-col")).toBeTruthy();
    expect(group.querySelector(".items-center")).toBeTruthy();
    expect(group.className).toMatch(/text-center/);
    expect(group.className).not.toMatch(/text-right/);

    const table = screen.getByRole("grid", { name: "Contact handles" });
    expect(table.querySelectorAll(".cursor-col-resize").length).toBeGreaterThanOrEqual(7);

    const headers = screen.getAllByRole("columnheader");
    const groupIndex = headers.findIndex((h) => /Group Messages/i.test(h.textContent ?? ""));
    expect(groupIndex).toBe(headers.length - 1);
  });

  it("moves the contact to trash and closes the drawer", async () => {
    get.mockResolvedValue(detail(1));
    const user = userEvent.setup();
    const onClose = vi.fn();

    render(
      <ContactDrawer
        variant="docked"
        contactId="1"
        preview={{
          id: "1",
          name: "Contact a",
          handles: ["+1555000a"],
          groups: [],
        }}
        onClose={onClose}
      />,
    );

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Contact a" })).toBeTruthy();
    });

    await user.click(screen.getByRole("button", { name: "Move to trash" }));

    await waitFor(() => {
      expect(trash).toHaveBeenCalledWith("1", expect.anything());
      expect(onClose).toHaveBeenCalled();
    });
  });

  it("shows an error and leaves the drawer open when trashing fails", async () => {
    get.mockResolvedValue(detail(1));
    trash.mockRejectedValue(new Error("Could not move this contact."));
    const user = userEvent.setup();
    const onClose = vi.fn();

    render(
      <ContactDrawer
        variant="docked"
        contactId="1"
        preview={{
          id: "1",
          name: "Contact a",
          handles: ["+1555000a"],
          groups: [],
        }}
        onClose={onClose}
      />,
    );

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Contact a" })).toBeTruthy();
    });

    await user.click(screen.getByRole("button", { name: "Move to trash" }));

    expect(await screen.findByText("Could not move this contact.")).toBeTruthy();
    expect(onClose).not.toHaveBeenCalled();
  });
});

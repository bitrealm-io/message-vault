/** @vitest-environment jsdom */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render as rtlRender, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "../lib/api";
import type { ContactDetail } from "../lib/contactDetail";
import { keys } from "../lib/queryKeys";
import { routeQueryKey } from "../lib/routeQueryKey";
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
  client.setQueryData(routeQueryKey(7, keys.contacts.detail(detail.id)), detail);
}

const get = vi.fn();
const post = vi.fn();

const trash = vi.fn();

vi.mock("../lib/serverApi", () => ({
  getContact: (...args: unknown[]) => get(...args),
  updateContact: (...args: unknown[]) => post(...args),
  trashContact: (...args: unknown[]) => trash(...args),
}));
function detail(id: number, overrides: Partial<ContactDetail> = {}): ContactDetail {
  return {
    id,
    name: `Contact ${id}`,
    unknown: false,
    last_modified: "2024-01-01T00:00:00Z",
    identities: [
      {
        address: `+1555000${id}`,
        service: "phone",
        start_date: "2020-01-01T00:00:00Z",
        end_date: "2024-01-01T00:00:00Z",
        conversations: 4,
        direct_messages: 42,
        group_messages: 7,
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
          addresses: a.identities.map((h) => h.address),
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
          addresses: ["+1555000b"],
          groups: ["Family"],
        }}
        onClose={() => {}}
      />,
    );

    const dialog = screen.getByRole("dialog", { name: "Contact b" });
    expect(dialog.getAttribute("aria-busy")).toBe("true");
    expect(screen.getByText("Family")).toBeTruthy();
    expect(screen.getByText("+1555000b")).toBeTruthy();

    const table = screen.getByRole("grid", { name: "Contact identities" });
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
      identities: [
        {
          address: "+15551212",
          service: "phone",
          start_date: "2021-06-01T00:00:00Z",
          end_date: "2025-01-01T00:00:00Z",
          conversations: 7,
          direct_messages: 99,
          group_messages: 11,
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
        preview={{ id: "1", name: a.name, addresses: ["+1555000a"], groups: a.groups }}
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
          addresses: ["+15551212"],
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

  it("names a contact with no preferred name by its first identity, in italics, under Unknown", async () => {
    seed(detail(5, { name: "", unknown: true, groups: [] }));

    render(<ContactDrawer variant="docked" contactId="5" preview={null} onClose={() => {}} />);

    const heading = await screen.findByRole("heading", { name: "+15550005" });
    expect(heading.querySelector("em")?.textContent).toBe("+15550005");
    expect(screen.getByRole("dialog", { name: "+15550005" })).toBeTruthy();
    expect(screen.getByText("Unknown")).toBeTruthy();
    expect(screen.queryByText("No groups")).toBeNull();
  });

  it("sets a preferred name upright and leaves Unknown off a known contact", async () => {
    seed(detail(6, { name: "Grace", groups: [] }));

    render(<ContactDrawer variant="docked" contactId="6" preview={null} onClose={() => {}} />);

    const heading = await screen.findByRole("heading", { name: "Grace" });
    expect(heading.querySelector("em")).toBeNull();
    expect(screen.queryByText("Unknown")).toBeNull();
    expect(screen.getByText("No groups")).toBeTruthy();
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

  it("says a contact could not be loaded, stops claiming to load, and loads it on Try again", async () => {
    get.mockRejectedValueOnce(new ApiError(500, "The server could not answer."));
    get.mockResolvedValueOnce(detail(26, { name: "Zed" }));
    const user = userEvent.setup();

    render(
      <ContactDrawer
        variant="overlay"
        contactId="26"
        preview={{ id: "26", name: "Zed" }}
        onClose={() => {}}
      />,
    );

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("This contact could not be loaded.");
    expect(alert.textContent).toContain("The server could not answer.");
    expect(screen.getByRole("dialog").getAttribute("aria-busy")).toBeNull();

    await user.click(screen.getByRole("button", { name: "Try again" }));

    await waitFor(() => {
      expect(screen.getByText("Group-26")).toBeTruthy();
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("dialog").getAttribute("aria-busy")).toBeNull();
  });

  it("says plainly that a contact no longer exists when the server answers 404 Not Found", async () => {
    get.mockRejectedValue(new ApiError(404, "No contact with id 26."));

    render(<ContactDrawer variant="overlay" contactId="26" preview={null} onClose={() => {}} />);

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("This contact is no longer in your contacts.");
    expect(screen.queryByText("Loading…")).toBeNull();
    expect(screen.getByRole("dialog").getAttribute("aria-busy")).toBeNull();
    expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
  });

  // The app's own :focus-visible outline loses to the outline-none utility, so
  // each close button draws the style guide's ring itself.
  it("draws the focus ring on the close button and sortable headers of a loaded contact", async () => {
    seed(detail(6, { name: "Grace", groups: [] }));
    render(<ContactDrawer variant="docked" contactId="6" preview={null} onClose={() => {}} />);
    await screen.findByRole("heading", { name: "Grace" });
    expect(screen.getByRole("button", { name: "Close" }).className).toContain(
      "focus-visible:ring-2 focus-visible:ring-accent",
    );
    const sortable = screen
      .getAllByRole("columnheader")
      .filter((header) => header.hasAttribute("aria-sort"));
    expect(sortable.length).toBeGreaterThan(0);
    for (const header of sortable) {
      expect(header.className).toContain("focus-visible:ring-2 focus-visible:ring-accent");
    }
  });

  it("draws the focus ring on the close button of a contact that failed to load", async () => {
    get.mockRejectedValue(new ApiError(404, "No contact with id 26."));
    render(<ContactDrawer variant="overlay" contactId="26" preview={null} onClose={() => {}} />);
    await screen.findByRole("alert");
    expect(screen.getByRole("button", { name: "Close" }).className).toContain(
      "focus-visible:ring-2 focus-visible:ring-accent",
    );
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
          addresses: ["+1555000b", "1555000b"],
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

    const table = screen.getByRole("grid", { name: "Contact identities" });
    // Header + one handle row + summary row.
    expect(table.querySelectorAll('[role="row"]').length).toBe(3);

    resolveDetail(
      detail(2, {
        name: "Contact b",
        groups: ["Family"],
        identities: [
          {
            address: "+1555000b",
            service: "phone",
            start_date: "2020-01-01T00:00:00Z",
            end_date: "2024-01-01T00:00:00Z",
            conversations: 4,
            direct_messages: 42,
            group_messages: 7,
          },
        ],
      }),
    );
    await waitFor(() => {
      expect(dialog.getAttribute("aria-busy")).toBeNull();
      expect(table.querySelectorAll('[role="row"]').length).toBe(3);
    });
  });

  it("stubs overlay addresses from thread preview while detail is pending", async () => {
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
          addresses: ["+1555000b"],
          handleCount: 1,
        }}
        onClose={() => {}}
      />,
    );

    expect(screen.getByRole("heading", { name: "Contact b" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "Loading…" })).toBeNull();
    expect(screen.getByText("+1555000b")).toBeTruthy();

    const table = screen.getByRole("grid", { name: "Contact identities" });
    expect(table.querySelectorAll('[role="row"]').length).toBe(3);

    resolveDetail(
      detail(2, {
        name: "Contact b",
        identities: [
          {
            address: "+1555000b",
            service: "phone",
            start_date: "2020-01-01T00:00:00Z",
            end_date: "2024-01-01T00:00:00Z",
            conversations: 4,
            direct_messages: 42,
            group_messages: 7,
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
          addresses: [],
          handleCount: 1,
        }}
        onClose={() => {}}
      />,
    );

    expect(screen.getByRole("heading", { name: "Mom" })).toBeTruthy();
    expect(screen.queryByText("Loading…")).toBeNull();

    const table = screen.getByRole("grid", { name: "Contact identities" });
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
          addresses: ["+15550001", "15550001", "+15550002", "15550002"],
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
    const table = screen.getByRole("grid", { name: "Contact identities" });
    expect(table.querySelectorAll('[role="row"]').length).toBe(4);

    resolveDetail(
      detail(2, {
        name: "Contact b",
        groups: ["Family"],
        identities: [
          {
            address: "+15550001",
            service: "phone",
            start_date: "2020-01-01T00:00:00Z",
            end_date: "2024-01-01T00:00:00Z",
            conversations: 1,
            direct_messages: 4,
            group_messages: 0,
          },
          {
            address: "+15550002",
            service: "phone",
            start_date: "2020-01-01T00:00:00Z",
            end_date: "2024-01-01T00:00:00Z",
            conversations: 2,
            direct_messages: 8,
            group_messages: 0,
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
          addresses: ["+1555000b"],
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
          addresses: ["+1555000a"],
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

  it("shows why an empty name was refused and keeps the editor open", async () => {
    post.mockRejectedValue(new Error("name must not be empty"));
    const user = userEvent.setup();
    const input = await openNameEditor(user);
    await user.clear(input);
    await user.keyboard("{Enter}");

    expect(await screen.findByRole("alert")).toHaveTextContent("name must not be empty");
    expect(post).toHaveBeenCalledWith("1", { name: "" });
    expect(screen.getByRole("textbox", { name: "Contact name" })).toBeTruthy();
  });

  it("aligns text headers left and number headers right, and keeps Remove last", async () => {
    get.mockResolvedValue(detail(1));
    render(
      <ContactDrawer
        variant="docked"
        contactId="1"
        preview={{
          id: "1",
          name: "Contact a",
          addresses: ["+1555000a"],
          groups: [],
        }}
        onClose={() => {}}
      />,
    );

    await waitFor(() => {
      expect(screen.getByRole("grid", { name: "Contact identities" })).toBeTruthy();
    });

    for (const name of [/Service/i, /^Identity/i]) {
      const header = screen.getByRole("columnheader", { name });
      expect(header.className).toMatch(/text-left/);
      expect(header.className).not.toMatch(/text-center|text-right/);
    }
    for (const name of [
      /First heard from/i,
      /Last heard from/i,
      /Conversations/i,
      /Direct messages/i,
      /Group messages/i,
    ]) {
      const header = screen.getByRole("columnheader", { name });
      expect(header.className).toMatch(/text-right/);
      expect(header.className).not.toMatch(/text-center|text-left/);
    }
    expect(screen.queryByRole("columnheader", { name: /Threads/i })).toBeNull();

    const table = screen.getByRole("grid", { name: "Contact identities" });
    expect(table.querySelectorAll(".cursor-col-resize").length).toBe(0);

    const headers = screen.getAllByRole("columnheader");
    expect(headers[headers.length - 1].textContent).toBe("");
    // Once the detail is in, the row carries its service and counts.
    await waitFor(() => expect(screen.getAllByText("42").length).toBeGreaterThan(0));
    const remove = screen.getByRole("button", { name: "Remove +15550001 (Text message)" });
    expect(remove.closest("[role=row]")?.lastElementChild).toContainElement(remove);
    expect(screen.getByText("Summary")).toBeTruthy();
  });

  it("counts a conversation two of the contact's identities share once in the Summary row", async () => {
    // A phone and an email take part in one group conversation, the contact's
    // only one: each identity counts it, and the contact has one (#1248).
    const shared = {
      start_date: "2024-06-01T12:00:00Z",
      end_date: "2024-06-01T12:00:00Z",
      conversations: 1,
      direct_messages: 0,
      group_messages: 2,
    };
    get.mockResolvedValue(
      detail(1, {
        identities: [
          { ...shared, address: "+15550001", service: "phone" },
          { ...shared, address: "sam@example.com", service: "email" },
        ],
        direct_conversations: 0,
        group_conversations: 1,
        total_messages: 4,
      }),
    );
    render(<ContactDrawer variant="docked" contactId="1" onClose={() => {}} />);

    const summary = (await screen.findByText("Summary")).closest("[role=row]");
    const cells = within(summary as HTMLElement).getAllByRole("gridcell");
    expect(cells.map((c) => c.textContent)).toEqual([
      "Summary",
      "2024-06-01",
      "2024-06-01",
      "1",
      "—",
      "4",
      "",
    ]);
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
          addresses: ["+1555000a"],
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
          addresses: ["+1555000a"],
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

  it("does not show a Move to trash error from one contact on the next one opened", async () => {
    get.mockImplementation(async (id: string) => detail(Number(id)));
    trash.mockRejectedValue(new Error("Trash refused."));
    const user = userEvent.setup();
    const onClose = vi.fn();

    const { rerender } = render(<ContactDrawer variant="docked" contactId="1" onClose={onClose} />);
    await screen.findByRole("heading", { name: "Contact 1" });
    await user.click(screen.getByRole("button", { name: "Move to trash" }));
    await screen.findByText("Trash refused.");

    rerender(<ContactDrawer variant="docked" contactId="2" onClose={onClose} />);
    await screen.findByRole("heading", { name: "Contact 2" });
    expect(screen.queryByText("Trash refused.")).toBeNull();
  });

  it("leaves the next contact open when a Move to trash pressed on the last one succeeds", async () => {
    get.mockImplementation(async (id: string) => detail(Number(id)));
    let finishTrash: () => void = () => {};
    trash.mockReturnValue(
      new Promise<void>((resolve) => {
        finishTrash = resolve;
      }),
    );
    const user = userEvent.setup();
    const onClose = vi.fn();

    const { rerender } = render(<ContactDrawer variant="docked" contactId="1" onClose={onClose} />);
    await screen.findByRole("heading", { name: "Contact 1" });
    await user.click(screen.getByRole("button", { name: "Move to trash" }));
    await waitFor(() => expect(trash).toHaveBeenCalledWith("1", expect.anything()));

    rerender(<ContactDrawer variant="docked" contactId="2" onClose={onClose} />);
    await screen.findByRole("heading", { name: "Contact 2" });
    // Contact 2 has nothing pending, so its button is live.
    expect(screen.getByRole("button", { name: "Move to trash" })).not.toBeDisabled();

    finishTrash();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(onClose).not.toHaveBeenCalled();
  });
});

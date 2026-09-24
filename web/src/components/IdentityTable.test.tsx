/** @vitest-environment jsdom */

import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import IdentityTable, { type IdentityRow } from "./IdentityTable";

afterEach(cleanup);

const rows: IdentityRow[] = [
  {
    address: "+15555550100",
    service: "phone",
    start_date: "2020-01-01T00:00:00Z",
    end_date: "2020-02-03T00:00:00Z",
    conversations: 2,
    direct_messages: 12,
    group_messages: 30,
  },
  {
    address: "someone.with.a.long.address@example.com",
    service: "email",
    start_date: null,
    end_date: null,
    conversations: 0,
    direct_messages: 0,
    group_messages: 0,
  },
  {
    address: "+15555550100",
    service: "whatsapp",
    start_date: "2021-05-05T00:00:00Z",
    end_date: "2021-05-06T00:00:00Z",
    conversations: 1,
    direct_messages: 3,
    group_messages: 0,
  },
];

const headers = () =>
  screen.getAllByRole("columnheader").map((h) => h.textContent?.replace(/[▲▼]/g, "").trim());
const identities = () =>
  screen.getAllByRole("rowheader").map((cell) => cell.textContent?.trim() ?? "");

describe("IdentityTable", () => {
  it("shows the eight columns, text left and numbers right, with the header aligned like its cells", () => {
    render(<IdentityTable rows={rows} onRemove={() => {}} />);

    expect(headers()).toEqual([
      "Service",
      "Identity",
      "First seen",
      "Last seen",
      "Conversations",
      "Direct messages",
      "Group messages",
      "",
    ]);
    const [service, identity, first, , conversations] = screen.getAllByRole("columnheader");
    expect(service.className).toContain("text-left");
    expect(identity.className).toContain("text-left");
    expect(first.className).toContain("text-right");
    expect(conversations.className).toContain("text-right");

    const row = screen.getAllByRole("row")[1];
    const cells = within(row).getAllByRole("gridcell");
    expect(cells[0].className).toContain("text-left");
    expect(cells[1].className).toContain("text-right");
    expect(cells[3].className).toContain("text-right");
    expect(cells[1].textContent).toBe("2020-01-01");
    expect(cells[3].textContent).toBe("2");
    expect(cells[5].textContent).toBe("30");
  });

  it("puts the sort arrow right after the label and shows it only on the sorted column", async () => {
    const user = userEvent.setup({ delay: null });
    render(<IdentityTable rows={rows} onRemove={() => {}} />);

    const identity = screen.getByRole("columnheader", { name: /^Identity/ });
    const arrow = identity.querySelector("[aria-hidden]");
    expect(arrow?.className).toContain("invisible");
    expect(arrow?.previousSibling?.textContent).toBe("Identity");

    await user.click(identity);
    expect(identity.querySelector("[aria-hidden]")?.className).not.toContain("invisible");
    expect(identities()).toEqual([
      "+15555550100",
      "+15555550100",
      "someone.with.a.long.address@example.com",
    ]);

    await user.click(identity);
    expect(identities()[0]).toBe("someone.with.a.long.address@example.com");

    await user.click(screen.getByRole("columnheader", { name: /Direct messages/ }));
    expect(identities()[0]).toBe("someone.with.a.long.address@example.com");
    expect(identity.querySelector("[aria-hidden]")?.className).toContain("invisible");
  });

  it("cuts a long identity with an ellipsis and keeps the whole value in the title", () => {
    render(<IdentityTable rows={rows} onRemove={() => {}} />);
    const cell = screen.getByRole("rowheader", { name: /someone/ });
    const text = within(cell).getByTitle("someone.with.a.long.address@example.com");
    expect(text.className).toContain("truncate");
  });

  it("shows a muted dash for a zero count or a missing date", () => {
    render(<IdentityTable rows={rows} onRemove={() => {}} />);
    const row = screen.getAllByRole("row")[2];
    const cells = within(row).getAllByRole("gridcell");
    expect(cells[1].textContent).toBe("—");
    expect(cells[3].textContent).toBe("—");
    expect(cells[5].textContent).toBe("—");
  });

  it("ends every row with an always visible Remove named after the identity and its service", async () => {
    const user = userEvent.setup({ delay: null });
    const onRemove = vi.fn();
    render(<IdentityTable rows={rows} onRemove={onRemove} />);

    const remove = screen.getByRole("button", {
      name: "Remove someone.with.a.long.address@example.com (Email)",
    });
    expect(remove.className).not.toMatch(/opacity-0/);
    const row = remove.closest("[role=row]");
    expect(row?.lastElementChild).toContainElement(remove);

    await user.click(remove);
    expect(onRemove).toHaveBeenCalledWith(rows[1]);
  });

  it("disables Remove while busy", () => {
    render(<IdentityTable rows={rows} busy onRemove={() => {}} />);
    expect(
      screen.getByRole("button", {
        name: "Remove someone.with.a.long.address@example.com (Email)",
      }),
    ).toBeDisabled();
  });

  it("makes the conversation count a link only when given somewhere to browse to", async () => {
    const user = userEvent.setup({ delay: null });
    const { unmount } = render(<IdentityTable rows={rows} onRemove={() => {}} />);
    expect(screen.queryByRole("button", { name: /Open 2 conversations/ })).not.toBeInTheDocument();
    unmount();

    const onBrowse = vi.fn();
    render(<IdentityTable rows={rows} onRemove={() => {}} onBrowse={onBrowse} />);
    await user.click(screen.getByRole("button", { name: "Open 2 conversations" }));
    expect(onBrowse).toHaveBeenCalledWith(rows[0]);
    // A zero count is never a link.
    expect(screen.queryByRole("button", { name: /Open 0/ })).not.toBeInTheDocument();
  });

  it("adds a Summary row only when asked, with the earliest, latest and the sums", () => {
    const { unmount } = render(<IdentityTable rows={rows} onRemove={() => {}} />);
    expect(screen.queryByText("Summary")).not.toBeInTheDocument();
    unmount();

    render(<IdentityTable rows={rows} totals onRemove={() => {}} />);
    const summary = screen.getByText("Summary").closest("[role=row]");
    const cells = within(summary as HTMLElement).getAllByRole("gridcell");
    expect(cells.map((c) => c.textContent)).toEqual([
      "Summary",
      "2020-01-01",
      "2021-05-06",
      "3",
      "15",
      "30",
      "",
    ]);
  });

  it("shows dashes for every count and date while loading", () => {
    render(<IdentityTable rows={rows} loading onRemove={() => {}} />);
    const row = screen.getAllByRole("row")[1];
    const cells = within(row).getAllByRole("gridcell");
    expect(cells.slice(1, 6).map((c) => c.textContent)).toEqual(["—", "—", "—", "—", "—"]);
    expect(
      screen.getByRole("button", { name: "Remove +15555550100 (Text message)" }),
    ).toBeDisabled();
  });

  it("says so instead of drawing a table when there are no identities", () => {
    render(<IdentityTable rows={[]} onRemove={() => {}} emptyText="No identities yet." />);
    expect(screen.queryByRole("grid")).not.toBeInTheDocument();
    expect(screen.getByText("No identities yet.")).toBeInTheDocument();
  });
});

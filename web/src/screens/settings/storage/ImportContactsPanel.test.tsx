/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getImportContacts } from "../../../lib/vaultApi";
import ImportContactsPanel from "./ImportContactsPanel";

vi.mock("../../../lib/vaultApi", () => ({
  getImportContacts: vi.fn(),
}));

const get = vi.mocked(getImportContacts);

describe("ImportContactsPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("asks for the contacts of the run it was given", async () => {
    get.mockResolvedValue({ items: [], total: 0, limit: 40, offset: 0 });
    render(<ImportContactsPanel importId={42} newCount={0} changedCount={0} />);
    expect(await screen.findByText("This import changed no contacts.")).toBeInTheDocument();
    expect(get).toHaveBeenCalledWith(42);
  });

  it("states the run's tally and lists its contacts", async () => {
    get.mockResolvedValue({
      items: [
        { id: 1, name: "Ada Lovelace", reason: "replaced_trashed" },
        { id: 2, name: "Grace Hopper", reason: "created" },
        { id: 3, name: "Mary Jackson", reason: "named" },
        { id: 4, name: "Katherine Johnson", reason: "handle_added" },
      ],
      total: 4,
      limit: 40,
      offset: 0,
    });
    render(<ImportContactsPanel importId={7} newCount={2} changedCount={2} />);
    expect(await screen.findByText("2 new, 2 changed")).toBeInTheDocument();
    expect(screen.getByText("Ada Lovelace")).toBeInTheDocument();
    expect(screen.getByText("New, replaces a trashed contact")).toBeInTheDocument();
    expect(screen.getByText("Grace Hopper")).toBeInTheDocument();
    expect(screen.getByText("New")).toBeInTheDocument();
    expect(screen.getByText("Mary Jackson")).toBeInTheDocument();
    expect(screen.getByText("Named")).toBeInTheDocument();
    expect(screen.getByText("Katherine Johnson")).toBeInTheDocument();
    expect(screen.getByText("Handle added")).toBeInTheDocument();
  });

  it("shows a contact the run found an address for but no name", async () => {
    get.mockResolvedValue({
      items: [{ id: 3, name: "", reason: "created" }],
      total: 1,
      limit: 40,
      offset: 0,
    });
    render(<ImportContactsPanel importId={9} newCount={1} changedCount={0} />);
    expect(await screen.findByText("(unknown)")).toBeInTheDocument();
  });

  it("shows the reason when the load fails", async () => {
    get.mockRejectedValue(new Error("no such import"));
    render(<ImportContactsPanel importId={11} newCount={0} changedCount={0} />);
    expect(await screen.findByText("no such import")).toBeInTheDocument();
  });
});

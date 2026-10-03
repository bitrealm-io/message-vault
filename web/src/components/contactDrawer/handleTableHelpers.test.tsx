/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import {
  Cell,
  ResizableTableContainer,
  Row,
  type SortDescriptor,
  Table,
  TableBody,
  TableHeader,
} from "react-aria-components";
import { afterEach, describe, expect, it } from "vitest";
import { SortableColumn } from "./handleTableHelpers";

afterEach(() => {
  cleanup();
});

function Harness() {
  const [sortDescriptor, setSortDescriptor] = useState<SortDescriptor | null>(null);
  return (
    <ResizableTableContainer>
      <Table
        aria-label="Contact handles"
        sortDescriptor={sortDescriptor ?? undefined}
        onSortChange={setSortDescriptor}
      >
        <TableHeader>
          <SortableColumn id="service" isRowHeader align="left" allowsResizing>
            Service
          </SortableColumn>
          <SortableColumn id="handle" align="left" allowsResizing>
            Identity
          </SortableColumn>
        </TableHeader>
        <TableBody>
          <Row id="r1">
            <Cell>Phone</Cell>
            <Cell>+15551212</Cell>
          </Row>
        </TableBody>
      </Table>
    </ResizableTableContainer>
  );
}

describe("SortableColumn", () => {
  it("defaults align to center so callers like CheckedContactsPanel stay unchanged", () => {
    render(
      <ResizableTableContainer>
        <Table aria-label="Checked contacts">
          <TableHeader>
            <SortableColumn id="start_date">First Seen</SortableColumn>
          </TableHeader>
          <TableBody>
            <Row id="r1">
              <Cell>2024-01-15</Cell>
            </Row>
          </TableBody>
        </Table>
      </ResizableTableContainer>,
    );
    const header = screen.getByRole("columnheader", { name: /First Seen/i });
    expect(header.className).toMatch(/text-center/);
    expect(header.className).not.toMatch(/text-left/);
  });

  it("renders left-aligned column headers with visible resizers", () => {
    const { container } = render(<Harness />);
    expect(screen.getByRole("grid", { name: "Contact handles" })).toBeTruthy();
    const service = screen.getByRole("columnheader", { name: /Service/i });
    expect(service.className).toMatch(/text-left/);
    // RAC ColumnResizer uses role="presentation"; identify by resize cursor class.
    const resizers = container.querySelectorAll(".cursor-col-resize");
    expect(resizers.length).toBeGreaterThanOrEqual(2);
  });

  it("pins the sort caret to the column edge, not the label", () => {
    render(
      <ResizableTableContainer>
        <Table aria-label="Contact handles">
          <TableHeader>
            <SortableColumn id="service" allowsResizing>
              Service
            </SortableColumn>
          </TableHeader>
          <TableBody>
            <Row id="r1">
              <Cell>Phone</Cell>
            </Row>
          </TableBody>
        </Table>
      </ResizableTableContainer>,
    );
    const header = screen.getByRole("columnheader", { name: /Service/i });
    const flexRow = header.querySelector(".relative.flex.w-full");
    const caret = Array.from(header.querySelectorAll("[aria-hidden='true']")).find(
      (el) => el.textContent === "▲" || el.textContent === "▼",
    );
    expect(caret).toBeTruthy();
    expect(caret?.parentElement).toBe(flexRow);
    expect(caret?.className).toMatch(/absolute/);
    expect(caret?.className).toMatch(/right-1/);
    expect(caret?.className).toMatch(/pointer-events-none/);
    expect(caret?.className).toMatch(/invisible/);
    expect(header.className).toMatch(/px-0/);
    const group = flexRow?.querySelector(".flex-1");
    expect(group?.className).toMatch(/px-4/);
    expect(group?.className).not.toMatch(/pr-4/);
    const label = caret?.parentElement?.querySelector("span.max-w-full");
    expect(label?.className).not.toMatch(/px-4/);
    expect(label?.className).not.toMatch(/pr-4/);
    expect(label?.contains(caret as Node)).toBe(false);
  });

  it("pins the resizer as the last child of a full-width flex row", () => {
    render(<Harness />);
    const service = screen.getByRole("columnheader", { name: /Service/i });
    const flexRow = service.querySelector(".relative.flex.w-full");
    expect(flexRow).toBeTruthy();
    const children = Array.from(flexRow?.children ?? []);
    expect(children.length).toBeGreaterThanOrEqual(2);
    const last = children[children.length - 1] as HTMLElement;
    expect(last.className).toMatch(/cursor-col-resize/);
    expect(last.className).toMatch(/absolute/);
    const group = children[0] as HTMLElement;
    expect(group.className).toMatch(/flex-1/);
  });

  it("keeps right-aligned headers off the edge caret", () => {
    render(
      <ResizableTableContainer>
        <Table aria-label="Checked contacts">
          <TableHeader>
            <SortableColumn id="conversations" align="right">
              Threads
            </SortableColumn>
          </TableHeader>
          <TableBody>
            <Row id="r1">
              <Cell>12</Cell>
            </Row>
          </TableBody>
        </Table>
      </ResizableTableContainer>,
    );
    const header = screen.getByRole("columnheader", { name: /Threads/i });
    const group = header.querySelector(".flex-1");
    expect(group?.className).toMatch(/pr-4/);
    expect(group?.className).not.toMatch(/px-4/);
  });

  it("accents the active sort column and flips direction on second click", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    const service = screen.getByRole("columnheader", { name: /Service/i });
    const identity = screen.getByRole("columnheader", { name: /Identity/i });

    await user.click(service);
    expect(service.getAttribute("aria-sort")).toBe("ascending");
    expect(service.textContent).toContain("▲");
    expect(service.querySelector(".text-accent")).toBeTruthy();
    expect(identity.getAttribute("aria-sort")).toBe("none");

    await user.click(service);
    expect(service.getAttribute("aria-sort")).toBe("descending");
    expect(service.textContent).toContain("▼");
  });
});

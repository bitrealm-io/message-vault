/** @vitest-environment jsdom */

import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import TimeZoneField from "./TimeZoneField";

const browser = vi.hoisted(() => ({ zone: "Etc/UTC" }));
vi.mock("../lib/timeZone", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/timeZone")>()),
  browserTimeZone: () => browser.zone,
}));

function field() {
  return screen.getByRole("combobox", { name: "Time zone" }) as HTMLInputElement;
}

describe("TimeZoneField", () => {
  afterEach(() => {
    cleanup();
    browser.zone = "Etc/UTC";
  });

  it("shows a stored zone as its row, not as an IANA name", () => {
    render(<TimeZoneField value="America/Chicago" onChange={() => {}} />);
    expect(field().value).toMatch(/Central Time/);
  });

  it("shows a stored zone that shares a row under its own name", () => {
    render(<TimeZoneField value="America/Indiana/Knox" onChange={() => {}} />);
    expect(field().value).toMatch(/America\/Indiana\/Knox$/);
  });

  it("finds a zone by a city and hands back its IANA name", async () => {
    const onChange = vi.fn();
    render(<TimeZoneField value="Etc/UTC" onChange={onChange} />);
    await userEvent.click(field());
    await userEvent.keyboard("dallas");
    expect(field().value).toBe("dallas");
    const options = within(screen.getByRole("listbox")).getAllByRole("option");
    expect(options).toHaveLength(1);
    await userEvent.click(options[0]);
    expect(onChange).toHaveBeenCalledWith("America/Chicago");
  });

  it("stores the zone the person found, whose past differs from its row's", async () => {
    // Knox kept Eastern time from 1991 to 2006; Chicago did not.
    const onChange = vi.fn();
    render(<TimeZoneField value="Etc/UTC" onChange={onChange} />);
    await userEvent.click(field());
    await userEvent.keyboard("knox");
    const [first] = within(screen.getByRole("listbox")).getAllByRole("option");
    await userEvent.click(first);
    expect(onChange).toHaveBeenCalledWith("America/Indiana/Knox");
  });

  it("stores this browser's zone as the browser names it", async () => {
    browser.zone = "America/Indiana/Knox";
    const onChange = vi.fn();
    render(<TimeZoneField value="Etc/UTC" onChange={onChange} />);
    await userEvent.click(field());
    const [first] = within(screen.getByRole("listbox")).getAllByRole("option");
    expect(first.textContent).toMatch(/America\/Indiana\/Knox$/);
    await userEvent.click(first);
    expect(onChange).toHaveBeenCalledWith("America/Indiana/Knox");
  });

  it("lists every zone under this browser's when nothing is typed", async () => {
    render(<TimeZoneField value="Etc/UTC" onChange={() => {}} />);
    await userEvent.click(field());
    const list = screen.getByRole("listbox");
    expect(within(list).getByText("This browser")).toBeTruthy();
    expect(within(list).getAllByRole("option").length).toBeGreaterThan(300);
  });

  it("says so when nothing matches, and keeps the zone when the person leaves", async () => {
    const onChange = vi.fn();
    render(<TimeZoneField value="America/Chicago" onChange={onChange} />);
    await userEvent.click(field());
    await userEvent.keyboard("qqqzzz");
    expect(screen.getByText("No time zone matches.")).toBeTruthy();
    await userEvent.tab();
    expect(field().value).toMatch(/Central Time/);
    expect(onChange).not.toHaveBeenCalled();
  });
});

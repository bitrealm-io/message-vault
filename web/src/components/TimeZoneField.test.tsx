/** @vitest-environment jsdom */

import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import TimeZoneField from "./TimeZoneField";

function field() {
  return screen.getByRole("combobox", { name: "Time zone" }) as HTMLInputElement;
}

describe("TimeZoneField", () => {
  afterEach(cleanup);

  it("shows the stored zone as its row, not as an IANA name", () => {
    render(<TimeZoneField value="America/Indiana/Knox" onChange={() => {}} />);
    expect(field().value).toMatch(/Central Time/);
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

/** @vitest-environment jsdom */

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import Checkbox from "./Checkbox";

describe("Checkbox", () => {
  afterEach(() => {
    cleanup();
  });

  it("reports the new checked value once per click", () => {
    const onChange = vi.fn();
    render(<Checkbox checked={false} onChange={onChange} aria-label="Select Ada" />);

    fireEvent.click(screen.getByRole("checkbox", { name: "Select Ada" }));

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith(true, expect.anything());
  });

  it("toggles from a click on its visible label, still only once", () => {
    const onChange = vi.fn();
    render(
      <Checkbox checked={false} onChange={onChange}>
        No name
      </Checkbox>,
    );

    fireEvent.click(screen.getByText("No name"));

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith(true, expect.anything());
  });

  it("takes its accessible name from the visible label", () => {
    render(
      <Checkbox checked={false} onChange={() => {}}>
        No name
      </Checkbox>,
    );
    expect(screen.getByRole("checkbox", { name: "No name" })).toBeTruthy();
  });

  it("sets the DOM-only indeterminate flag", () => {
    render(<Checkbox checked={false} indeterminate onChange={() => {}} aria-label="Select all" />);
    const box = screen.getByRole("checkbox", { name: "Select all" }) as HTMLInputElement;
    expect(box.indeterminate).toBe(true);
  });

  it("is never both checked and indeterminate", () => {
    render(<Checkbox checked indeterminate onChange={() => {}} aria-label="Select all" />);
    const box = screen.getByRole("checkbox", { name: "Select all" }) as HTMLInputElement;
    expect(box.checked).toBe(true);
    expect(box.indeterminate).toBe(false);
  });

  it("passes disabled through to the input", () => {
    render(<Checkbox checked={false} disabled onChange={() => {}} aria-label="Select Ada" />);
    expect(screen.getByRole("checkbox", { name: "Select Ada" })).toBeDisabled();
  });
});

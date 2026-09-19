// @vitest-environment jsdom
import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { contactLabelText } from "../lib/contactLabel";
import ContactLabel from "./ContactLabel";

describe("contactLabelText", () => {
  it("is the preferred name when there is one", () => {
    expect(contactLabelText(" Grace Hopper ", ["+15550001"])).toBe("Grace Hopper");
  });

  it("is the first identity when there is no preferred name", () => {
    expect(contactLabelText("", ["", "+15550001", "grace@example.com"])).toBe("+15550001");
  });

  it("is empty for a contact with neither", () => {
    expect(contactLabelText(" ", undefined)).toBe("");
  });
});

describe("ContactLabel", () => {
  it("sets an identity standing in for the name in italics", () => {
    const { container } = render(<ContactLabel name="" handles={["+15550001"]} />);
    expect(container.querySelector("em")?.textContent).toBe("+15550001");
  });

  it("sets a preferred name upright", () => {
    const { container } = render(<ContactLabel name="Grace" handles={["+15550001"]} />);
    expect(container.querySelector("em")).toBeNull();
    expect(container.textContent).toBe("Grace");
  });
});

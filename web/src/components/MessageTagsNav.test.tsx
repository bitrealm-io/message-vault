/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { mockedAuth, Providers } from "../test/providers";
import MessageTagsNav from "./MessageTagsNav";

vi.mock("../lib/auth", () => ({ useAuth: () => mockedAuth }));

afterEach(() => {
  cleanup();
});

function renderNav(path: string) {
  return render(
    <Providers>
      <MemoryRouter initialEntries={[path]}>
        <MessageTagsNav tags={["Work"]} />
      </MemoryRouter>
    </Providers>,
  );
}

function classTokens(el: Element): string[] {
  return el.className.split(/\s+/).filter(Boolean);
}

describe("MessageTagsNav", () => {
  it("titles the section Message Tags", () => {
    renderNav("/");
    expect(screen.getByRole("button", { name: "Message Tags" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Create message tag" })).toBeTruthy();
  });

  it("keeps the No tag active fill from navGlyphRowClass", () => {
    renderNav("/no-tag");
    const btn = screen.getByRole("button", { name: "No tag" });
    const tokens = classTokens(btn);
    expect(tokens).toContain("bg-hover");
    expect(tokens).toContain("font-semibold");
    expect(tokens).toContain("px-0");
    expect(tokens).not.toContain("bg-transparent");
  });

  it("puts tag and No tag icons in the shared 15px leading slot", () => {
    renderNav("/");
    const work = screen.getByRole("button", { name: "Work" });
    const noTag = screen.getByRole("button", { name: "No tag" });
    expect(work.querySelector('[class*="size-[15px]"]')).not.toBeNull();
    expect(noTag.querySelector('[class*="size-[15px]"]')).not.toBeNull();
  });

  it("indents nested rows so icons line up with the heading title", () => {
    renderNav("/");
    const work = screen.getByRole("button", { name: "Work" });
    expect(work.className).toContain("pl-[calc(15px+0.5rem)]");
    expect(work.className).toContain("self-stretch");
    const noTagInner = screen
      .getByRole("button", { name: "No tag" })
      .querySelector('[class*="pl-[calc(15px+0.5rem)]"]');
    expect(noTagInner).not.toBeNull();
    expect(noTagInner?.className).toContain("self-stretch");
  });

  it("closes the tag options menu on Escape", async () => {
    const user = userEvent.setup();
    renderNav("/");
    await user.click(screen.getByRole("button", { name: "Tag options for Work" }));
    expect(screen.getByRole("menuitem", { name: "Rename…" })).toBeTruthy();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menuitem", { name: "Rename…" })).toBeNull();
  });
});

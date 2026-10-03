/** @vitest-environment jsdom */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
const save = vi.fn();
const isTauri = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: (...args: unknown[]) => save(...args),
}));
vi.mock("./tauri-check", () => ({
  isTauri: () => isTauri(),
}));

import { saveTextFile } from "./saveTextFile";

describe("saveTextFile", () => {
  beforeEach(() => {
    invoke.mockReset();
    save.mockReset();
    isTauri.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("downloads the text under the given name in a browser", async () => {
    isTauri.mockReturnValue(false);
    const created: Blob[] = [];
    URL.createObjectURL = vi.fn((blob: Blob) => {
      created.push(blob);
      return "blob:address-book";
    });
    URL.revokeObjectURL = vi.fn();
    const clicked: { href: string; download: string }[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (
      this: HTMLAnchorElement,
    ) {
      clicked.push({ href: this.href, download: this.download });
    });

    expect(await saveTextFile("address-book.csv", "a,b\n", "text/csv")).toBe(true);

    expect(clicked).toEqual([{ href: "blob:address-book", download: "address-book.csv" }]);
    expect(created[0].type).toBe("text/csv");
    expect(await created[0].text()).toBe("a,b\n");
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:address-book");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("has the desktop app ask where to save and write the file, with no path from the window", async () => {
    isTauri.mockReturnValue(true);
    invoke.mockResolvedValue(true);

    expect(await saveTextFile("address-book.csv", "a,b\n", "text/csv")).toBe(true);

    expect(save).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledWith("save_text_file", {
      fileName: "address-book.csv",
      contents: "a,b\n",
    });
  });

  it("reports false when the desktop app's dialog is closed without a choice", async () => {
    isTauri.mockReturnValue(true);
    invoke.mockResolvedValue(false);

    expect(await saveTextFile("address-book.csv", "a,b\n", "text/csv")).toBe(false);
  });
});

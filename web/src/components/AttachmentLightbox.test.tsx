/** @vitest-environment jsdom */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fetchAssetObjectUrl } from "../lib/serverApi";
import AttachmentLightbox, { type LightboxItem } from "./AttachmentLightbox";

vi.mock("../lib/serverApi", () => ({
  fetchAssetObjectUrl: vi.fn().mockResolvedValue("blob:mock-url"),
}));

const items: LightboxItem[] = [
  {
    attachment: {
      path: "first.png",
      original_name: "first.png",
      mime_type: "image/png",
      sha256: "aaa",
      is_sticker: false,
      transcription: null,
    },
  },
  {
    attachment: {
      path: "second.png",
      original_name: "second.png",
      mime_type: "image/png",
      sha256: "bbb",
      is_sticker: false,
      transcription: null,
    },
  },
];

beforeEach(() => {
  vi.mocked(fetchAssetObjectUrl).mockResolvedValue("blob:mock-url");
});

afterEach(() => {
  vi.clearAllMocks();
});

/** Render the viewer over `items`, with a spy on each callback. */
function open(currentIndex = 0, over: LightboxItem[] = items) {
  const spies = { onClose: vi.fn(), onPrev: vi.fn(), onNext: vi.fn() };
  render(<AttachmentLightbox items={over} currentIndex={currentIndex} {...spies} />);
  return spies;
}

describe("AttachmentLightbox", () => {
  it("moves to the next attachment when ArrowRight is pressed", async () => {
    const onNext = vi.fn();
    const onPrev = vi.fn();
    render(
      <AttachmentLightbox
        items={items}
        currentIndex={0}
        onClose={() => {}}
        onPrev={onPrev}
        onNext={onNext}
      />,
    );
    await screen.findByRole("img", { name: "first.png" });

    fireEvent.keyDown(document.body, { key: "ArrowRight" });

    expect(onNext).toHaveBeenCalledTimes(1);
    expect(onPrev).not.toHaveBeenCalled();
  });

  it("moves to the previous attachment when ArrowLeft is pressed", async () => {
    const onPrev = vi.fn();
    const onNext = vi.fn();
    render(
      <AttachmentLightbox
        items={items}
        currentIndex={1}
        onClose={() => {}}
        onPrev={onPrev}
        onNext={onNext}
      />,
    );
    await screen.findByRole("img", { name: "second.png" });

    fireEvent.keyDown(document.body, { key: "ArrowLeft" });

    expect(onPrev).toHaveBeenCalledTimes(1);
    expect(onNext).not.toHaveBeenCalled();
  });
});

/**
 * The three buttons on the viewer.
 *
 * Only the arrow keys were tested, so the buttons a person actually clicks
 * were not: a viewer whose Next button called `onPrev`, or whose Close did
 * nothing, passed the file. These press each one.
 */
describe("AttachmentLightbox buttons", () => {
  it("moves forward and back with the on-screen arrows", async () => {
    const user = userEvent.setup();
    const spies = open(0);
    await screen.findByRole("img", { name: "first.png" });

    await user.click(screen.getByRole("button", { name: "Next attachment" }));
    expect(spies.onNext).toHaveBeenCalledTimes(1);
    expect(spies.onPrev).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Previous attachment" }));
    expect(spies.onPrev).toHaveBeenCalledTimes(1);
    expect(spies.onNext).toHaveBeenCalledTimes(1);
  });

  it("closes on the close button", async () => {
    const user = userEvent.setup();
    const spies = open(0);
    await screen.findByRole("img", { name: "first.png" });

    await user.click(screen.getByRole("button", { name: "Close attachment viewer" }));

    expect(spies.onClose).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape, which is the way out people try first", async () => {
    const spies = open(0);
    await screen.findByRole("img", { name: "first.png" });

    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });

    expect(spies.onClose).toHaveBeenCalled();
  });

  it("hides the arrows when there is only one attachment", async () => {
    open(0, [items[0]]);
    await screen.findByRole("img", { name: "first.png" });

    expect(screen.queryByRole("button", { name: "Next attachment" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Previous attachment" })).not.toBeInTheDocument();
    // Close is always there: it is the only way out that does not need a key.
    expect(screen.getByRole("button", { name: "Close attachment viewer" })).toBeInTheDocument();
  });

  it("says which of how many is on screen", async () => {
    open(1);
    await screen.findByRole("img", { name: "second.png" });

    expect(screen.getByText("2 / 2")).toBeInTheDocument();
  });
});

/**
 * Opening an attachment gives the original, at its full quality, even when
 * the conversation showed a preview. Only bytes the browser cannot draw fall
 * back to the preview, so a HEIC photo does not open as a broken image.
 */
describe("AttachmentLightbox and previews", () => {
  const heic: LightboxItem = {
    attachment: {
      original_name: "IMG_0001.heic",
      mime_type: "image/heic",
      sha256: "ccc",
      preview_mime_type: "image/jpeg",
    },
  };

  it("opens the original of an attachment that has a preview", async () => {
    open(0, [heic]);
    await screen.findByRole("img", { name: "IMG_0001.heic" });

    expect(fetchAssetObjectUrl).toHaveBeenCalledTimes(1);
    expect(vi.mocked(fetchAssetObjectUrl).mock.calls[0][1]?.preview).toBe(false);
  });

  it("shows the preview when the browser cannot draw the original", async () => {
    vi.mocked(fetchAssetObjectUrl).mockImplementation(async (_sha, options) =>
      options?.preview ? "blob:preview" : "blob:original",
    );
    open(0, [heic]);
    const original = await screen.findByRole("img", { name: "IMG_0001.heic" });
    expect(original).toHaveAttribute("src", "blob:original");

    fireEvent.error(original);

    await waitFor(() =>
      expect(screen.getByRole("img", { name: "IMG_0001.heic" })).toHaveAttribute(
        "src",
        "blob:preview",
      ),
    );
  });

  it("asks for no preview when the attachment has none", async () => {
    open(0);
    fireEvent.error(await screen.findByRole("img", { name: "first.png" }));

    expect(fetchAssetObjectUrl).toHaveBeenCalledTimes(1);
    expect(vi.mocked(fetchAssetObjectUrl).mock.calls[0][1]?.preview).toBe(false);
  });
});

describe("AttachmentLightbox while the file is not there", () => {
  it("says it is loading rather than showing a broken image", () => {
    vi.mocked(fetchAssetObjectUrl).mockReturnValue(new Promise(() => {}));
    open(0);

    expect(screen.getByText("Loading…")).toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });

  it("says the attachment could not be loaded when the server refuses it", async () => {
    vi.mocked(fetchAssetObjectUrl).mockRejectedValue(new Error("404"));
    open(0);

    expect(await screen.findByText("Failed to load attachment")).toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });

  it("renders nothing at all when the index names no attachment", () => {
    const spies = { onClose: vi.fn(), onPrev: vi.fn(), onNext: vi.fn() };
    const { container } = render(<AttachmentLightbox items={[]} currentIndex={0} {...spies} />);

    expect(container).toBeEmptyDOMElement();
  });
});

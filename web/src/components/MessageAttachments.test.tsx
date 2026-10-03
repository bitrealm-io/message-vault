/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fetchAssetObjectUrl } from "../lib/serverApi";
import type { Message, MessageAttachment } from "../lib/types";
import MessageAttachments from "./MessageAttachments";

vi.mock("../lib/serverApi", () => ({
  fetchAssetObjectUrl: vi.fn(),
}));

beforeEach(() => {
  vi.mocked(fetchAssetObjectUrl).mockResolvedValue("blob:mock-url");
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function message(attachments: MessageAttachment[]): Message {
  return {
    id: 1,
    source: "imessage",
    service: "iMessage",
    guid: null,
    timestamp: "2026-08-11T15:04:00Z",
    is_from_me: false,
    is_announcement: false,
    is_reply: false,
    num_replies: 0,
    sort_order: 0,
    sender: "+1555",
    subject: null,
    text: null,
    attachments,
    tapbacks: [],
    conversation: {
      id: 1,
      chat_identifier: "x",
      conversation_type: "individual",
      group_title: null,
      participants: [{ identity: "+1555", name: "Ada", contact_id: null }],
    },
  };
}

/** Which bytes the last fetch asked for: the preview or the original. */
function fetched(): { sha256: string; source: string; preview: boolean | undefined } {
  const [sha256, source, options] = vi.mocked(fetchAssetObjectUrl).mock.calls.at(-1) ?? [];
  return { sha256: sha256 ?? "", source: source ?? "", preview: options?.preview };
}

/**
 * A HEIC photo or an HEVC video is bytes most browsers cannot draw. The
 * server's preview is what makes it visible, so the conversation must ask for
 * the preview whenever the attachment has one, and for the original when it
 * does not.
 */
describe("MessageAttachments and previews", () => {
  it("shows a photo's preview when it has one", async () => {
    render(
      <MessageAttachments
        message={message([
          {
            original_name: "IMG_0001.heic",
            mime_type: "image/heic",
            sha256: "aaa",
            preview_mime_type: "image/jpeg",
          },
        ])}
      />,
    );

    expect(await screen.findByRole("img", { name: "IMG_0001.heic" })).toHaveAttribute(
      "src",
      "blob:mock-url",
    );
    expect(fetched()).toEqual({ sha256: "aaa", source: "imessage", preview: true });
  });

  it("shows a photo's original when it has no preview", async () => {
    render(
      <MessageAttachments
        message={message([{ original_name: "cat.png", mime_type: "image/png", sha256: "bbb" }])}
      />,
    );

    await screen.findByRole("img", { name: "cat.png" });
    expect(fetched()).toEqual({ sha256: "bbb", source: "imessage", preview: false });
  });

  it("plays a video's preview, in the preview's type, when it has one", async () => {
    render(
      <MessageAttachments
        message={message([
          {
            original_name: "clip.mov",
            mime_type: "video/quicktime",
            sha256: "ccc",
            preview_mime_type: "video/mp4",
          },
        ])}
      />,
    );

    const video = await screen.findByLabelText("clip.mov");
    expect(video.querySelector("source")).toHaveAttribute("type", "video/mp4");
    expect(fetched()).toEqual({ sha256: "ccc", source: "imessage", preview: true });
  });

  it("plays a video's original when it has no preview", async () => {
    render(
      <MessageAttachments
        message={message([{ original_name: "clip.mp4", mime_type: "video/mp4", sha256: "ddd" }])}
      />,
    );

    const video = await screen.findByLabelText("clip.mp4");
    expect(video.querySelector("source")).toHaveAttribute("type", "video/mp4");
    expect(fetched()).toEqual({ sha256: "ddd", source: "imessage", preview: false });
  });

  it("shows a preview for a photo whose own type is not known", async () => {
    render(
      <MessageAttachments
        message={message([
          { original_name: "IMG_0002", sha256: "eee", preview_mime_type: "image/jpeg" },
        ])}
      />,
    );

    await screen.findByRole("img", { name: "IMG_0002" });
    expect(fetched()).toEqual({ sha256: "eee", source: "imessage", preview: true });
  });
});

describe("a video with no stored file", () => {
  it("shows a file chip, as a photo with no digest does", () => {
    render(
      <MessageAttachments
        message={message([
          { original_name: "clip.mov", mime_type: "video/quicktime", sha256: null },
        ])}
      />,
    );
    expect(screen.getByText("clip.mov")).toBeInTheDocument();
  });
});

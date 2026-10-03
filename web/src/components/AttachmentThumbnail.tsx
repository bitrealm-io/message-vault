import { useAssetObjectUrl } from "../hooks/useAssetObjectUrl";
import { hasPreview, shownMimeType } from "../lib/attachmentPreview";
import { missingAttachmentChipLabel } from "../lib/missingAttachmentLabel";
import type { MessageAttachment } from "../lib/types";
import PlainButton from "./PlainButton";

export default function AttachmentThumbnail({
  attachment,
  source,
  onClick,
}: {
  attachment: MessageAttachment;
  source: string;
  onClick: () => void;
}) {
  const isMissing = Boolean(attachment.missing_reason);
  // Playable videos never reach here — MessageAttachments routes them to VideoPlayer.
  const isImage = shownMimeType(attachment)?.startsWith("image/");
  const wantsMedia = Boolean(!isMissing && attachment.sha256 && isImage);
  const { url, loading, error } = useAssetObjectUrl(
    wantsMedia ? attachment.sha256 : null,
    wantsMedia ? source : null,
    hasPreview(attachment),
  );

  if (isMissing) {
    return (
      <div className="mt-1.5 flex items-center gap-2 rounded bg-elevated px-2 py-2 text-[0.813rem] text-muted">
        <span>📎</span>
        <span>{missingAttachmentChipLabel(attachment)}</span>
      </div>
    );
  }

  // No renderable asset (missing digest) or an unknown file type — show a file chip
  if (!attachment.sha256 || !isImage) {
    return (
      <div className="mt-1.5 flex items-center gap-2 rounded bg-elevated px-2 py-2 text-[0.813rem]">
        <span>📎</span>
        <span className="text-text">{attachment.original_name || "attachment"}</span>
      </div>
    );
  }

  if (error) {
    return (
      <div className="mt-1.5 flex items-center gap-2 rounded bg-elevated px-2 py-2 text-[0.813rem] text-muted">
        <span>📎</span>
        <span>{attachment.original_name || "attachment"} (failed to load)</span>
      </div>
    );
  }

  if (loading || !url) {
    return (
      <div className="mt-1.5 flex h-[120px] max-w-[280px] items-center justify-center rounded-md bg-elevated text-[0.75rem] text-muted">
        Loading…
      </div>
    );
  }

  return (
    <PlainButton
      onPress={onClick}
      className="mt-1.5 block max-w-[280px] cursor-pointer overflow-hidden rounded-md border border-border bg-transparent p-0 text-left"
    >
      <img
        src={url}
        alt={attachment.original_name || "attachment"}
        loading="lazy"
        className="block h-auto max-h-[280px] w-auto max-w-[280px]"
      />
    </PlainButton>
  );
}

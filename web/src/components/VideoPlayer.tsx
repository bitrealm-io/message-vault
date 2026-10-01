import { useAssetObjectUrl } from "../hooks/useAssetObjectUrl";
import { hasPreview, shownMimeType } from "../lib/attachmentPreview";
import type { MessageAttachment } from "../lib/types";

export default function VideoPlayer({
  attachment,
  source,
}: {
  attachment: MessageAttachment;
  source: string;
}) {
  const { url, loading, error } = useAssetObjectUrl(
    attachment.sha256,
    source,
    hasPreview(attachment),
  );
  if (!attachment.sha256) return null;
  if (error) {
    return <div className="mt-1.5 text-[0.75rem] text-muted">Video failed to load</div>;
  }
  if (loading || !url) {
    return (
      <div className="mt-1.5 flex h-[160px] max-w-[400px] items-center justify-center rounded-md bg-elevated text-[0.75rem] text-muted">
        Loading video…
      </div>
    );
  }

  return (
    <div className="mt-1.5 max-w-[400px]">
      <video
        controls
        preload="metadata"
        className="w-full rounded-md"
        aria-label={attachment.original_name || "Video attachment"}
      >
        <source src={url} type={shownMimeType(attachment) || undefined} />
        <track kind="captions" />
      </video>
    </div>
  );
}

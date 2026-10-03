import { useLayoutEffect, useMemo, useRef } from "react";
import MessageBubble from "../../components/MessageBubble";
import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { useTimeZone } from "../../lib/timeZone";
import type { Message, MessageAttachment } from "../../lib/types";
import { threadRows } from "./threadLayout";
import type { Landing } from "./useConversationMessages";

/** How near an end of the thread, in pixels, the next page starts loading. */
const LOAD_MARGIN = 600;

/** Within this many pixels of the bottom, the thread counts as at the bottom and stays there. */
const BOTTOM_SLACK = 8;

/**
 * The conversation, the way a phone shows it (#1391): oldest at the top,
 * newest at the bottom, a separator for each day, and older and newer
 * messages loading as the person scrolls toward either end.
 *
 * Reading an older page puts it above what is on screen, so the scroll
 * position moves down by its height and the person stays on the message
 * they were reading. The browser's own scroll anchoring would do this in
 * Chromium and Firefox but not in WebKit, which the desktop app runs in on
 * macOS and Linux, so it is done here and the browser's is turned off.
 */
export default function MessageThread({
  messages,
  loading,
  error,
  findTerm,
  highlightId,
  isGroup,
  hasOlder,
  hasNewer,
  loadingOlder,
  loadingNewer,
  onLoadOlder,
  onLoadNewer,
  landing,
  jumping = false,
  onAttachmentClick,
}: {
  messages: Message[];
  loading: boolean;
  error: unknown;
  /** The Find term, drawn highlighted in every message; empty when Find is closed. */
  findTerm: string;
  /** The message a jump was for, drawn as the current match. */
  highlightId: number | null;
  /** In a group, a run's first message carries its sender's name. */
  isGroup: boolean;
  hasOlder: boolean;
  hasNewer: boolean;
  loadingOlder: boolean;
  loadingNewer: boolean;
  onLoadOlder: () => void;
  onLoadNewer: () => void;
  landing: Landing;
  /** The messages on screen are the last place's, while a jump loads. */
  jumping?: boolean;
  onAttachmentClick: (att: MessageAttachment, source: string) => void;
}) {
  const zone = useTimeZone();
  const rows = useMemo(() => threadRows(messages, zone), [messages, zone]);
  const scrollRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  /** The first message drawn and where it was, to keep it in place when older ones arrive above. */
  const topAnchor = useRef<{ id: number; top: number } | null>(null);
  const landedSeq = useRef(-1);
  const atBottom = useRef(false);

  // Read more when the person is near an end, or when what is loaded does not
  // fill the panel. Kept in a ref so the scroll handler and the layout
  // effect read this render's state.
  const loadNearEnds = useRef(() => {});
  loadNearEnds.current = () => {
    const el = scrollRef.current;
    if (!el || jumping) return;
    if (hasOlder && !loadingOlder && el.scrollTop < LOAD_MARGIN) onLoadOlder();
    if (hasNewer && !loadingNewer && el.scrollHeight - el.scrollTop - el.clientHeight < LOAD_MARGIN)
      onLoadNewer();
  };

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const first = rows[0]?.message.id;

    // Older messages arrived above the one that was first: move down by
    // however far that message moved, so it stays where the person saw it.
    const anchor = topAnchor.current;
    if (anchor && first !== anchor.id && landedSeq.current === landing.seq) {
      const node = document.getElementById(`row-${anchor.id}`);
      if (node) el.scrollTop += node.offsetTop - anchor.top;
    }

    // A jump lands once the message it is for is drawn, and not on the
    // messages of the last place still on screen while it loads.
    if (landedSeq.current !== landing.seq && !jumping && rows.length > 0) {
      if (landing.to === "bottom") {
        el.scrollTop = el.scrollHeight;
        landedSeq.current = landing.seq;
      } else {
        const node = document.getElementById(`row-${landing.to.id}`);
        if (node) {
          node.scrollIntoView({ block: landing.to.align });
          landedSeq.current = landing.seq;
        }
      }
    }
    atBottom.current =
      !hasNewer && el.scrollHeight - el.scrollTop - el.clientHeight <= BOTTOM_SLACK;

    const firstNode = first === undefined ? null : document.getElementById(`row-${first}`);
    topAnchor.current =
      first === undefined || !firstNode ? null : { id: first, top: firstNode.offsetTop };
    loadNearEnds.current();
  });

  // An image that loads after the thread drew grows it. At the bottom, the
  // thread stays at the bottom, the way a phone keeps the newest in view.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    const content = contentRef.current;
    if (!el || !content || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      if (atBottom.current) el.scrollTop = el.scrollHeight;
      const first = topAnchor.current;
      const node = first ? document.getElementById(`row-${first.id}`) : null;
      if (first && node) first.top = node.offsetTop;
    });
    observer.observe(content);
    return () => observer.disconnect();
  }, []);

  const term = findTerm.trim() || undefined;

  return (
    <div
      ref={scrollRef}
      onScroll={() => {
        const el = scrollRef.current;
        if (el) {
          atBottom.current =
            !hasNewer && el.scrollHeight - el.scrollTop - el.clientHeight <= BOTTOM_SLACK;
        }
        loadNearEnds.current();
      }}
      className="min-h-0 flex-1 overflow-auto [overflow-anchor:none]"
    >
      <div ref={contentRef} className="pb-3">
        {loading ? (
          <div className="p-4 text-[0.813rem] text-muted">Loading…</div>
        ) : error && rows.length === 0 ? (
          <div className="p-4 text-[0.813rem] text-danger">
            {apiErrorMessage(error, "Could not load messages.")}
          </div>
        ) : rows.length === 0 ? (
          <div className="p-4 text-[0.813rem] text-muted">No messages in this conversation</div>
        ) : (
          <>
            {hasOlder ? (
              <div className="pt-2 pb-1 text-center text-[0.75rem] text-muted">
                {loadingOlder ? "Loading older messages…" : "Scroll up for older messages"}
              </div>
            ) : null}
            {rows.map(({ message, day, startsRun }) => (
              <div key={message.id} id={`row-${message.id}`}>
                {day ? (
                  <div className="mx-4 mt-3.5 mb-1.5 flex items-center gap-2.5 text-[0.75rem] text-muted before:flex-1 before:border-t before:border-border before:content-[''] after:flex-1 after:border-t after:border-border after:content-['']">
                    <span>{day}</span>
                  </div>
                ) : null}
                <MessageBubble
                  message={message}
                  highlight={term}
                  isActive={highlightId === message.id}
                  showSender={isGroup && startsRun}
                  onAttachmentClick={onAttachmentClick}
                />
              </div>
            ))}
            {hasNewer ? (
              <div className="pt-1 pb-2 text-center text-[0.75rem] text-muted">
                {loadingNewer ? "Loading newer messages…" : "Scroll down for newer messages"}
              </div>
            ) : null}
            {error ? (
              <div className="px-4 py-2 text-[0.813rem] text-danger">
                {apiErrorMessage(error, "Could not load messages.")}
              </div>
            ) : null}
          </>
        )}
      </div>
    </div>
  );
}

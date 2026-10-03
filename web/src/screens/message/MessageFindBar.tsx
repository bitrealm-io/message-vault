import { useEffect, useRef } from "react";
import PlainButton from "../../components/PlainButton";

const STEP_CLASS =
  "cursor-pointer rounded border border-border bg-panel px-1.5 py-0.5 text-[0.75rem] text-text hover:bg-hover disabled:cursor-not-allowed disabled:opacity-40";

/**
 * Find in conversation (#1391). The term runs on the server (`GET /v1/messages`
 * with `in:#id`), so the count is every match in the conversation. Typing
 * jumps to the newest match; ▲ and ▼ step to the older and newer one, with the
 * messages around it; ✕ closes Find and leaves the thread where it is.
 */
export default function MessageFindBar({
  findTerm,
  onFindTermChange,
  matchCount,
  matchPosition,
  searching,
  onPrevMatch,
  onNextMatch,
  onClose,
}: {
  findTerm: string;
  onFindTermChange: (value: string) => void;
  /** Matches in the whole conversation, from the server. */
  matchCount: number;
  /** Zero-based position of the current match, counted from the newest. */
  matchPosition: number;
  /** The matches for the term are still being read. */
  searching: boolean;
  /** ▲: the older match. */
  onPrevMatch: () => void;
  /** ▼: the newer match. */
  onNextMatch: () => void;
  onClose: () => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const typed = findTerm.trim().length > 0;
  return (
    <div className="flex items-center gap-2 border-b border-border px-4 py-1.5">
      <input
        ref={inputRef}
        type="text"
        value={findTerm}
        aria-label="Find in conversation"
        onChange={(e) => onFindTermChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && matchCount > 0) {
            if (e.shiftKey) onNextMatch();
            else onPrevMatch();
          }
          if (e.key === "Escape") onClose();
        }}
        placeholder="Find in conversation…"
        className="box-border min-w-0 flex-1 rounded border border-border bg-bg px-2 py-1 text-[0.813rem] text-text"
      />
      {typed ? (
        <span className="whitespace-nowrap text-[0.75rem] text-muted" aria-live="polite">
          {matchCount > 0
            ? `${matchPosition + 1} of ${matchCount}`
            : searching
              ? "Finding…"
              : "No matches"}
        </span>
      ) : null}
      <PlainButton
        aria-label="Older match"
        title="Older match"
        isDisabled={matchCount === 0}
        onPress={onPrevMatch}
        className={STEP_CLASS}
      >
        ▲
      </PlainButton>
      <PlainButton
        aria-label="Newer match"
        title="Newer match"
        isDisabled={matchCount === 0}
        onPress={onNextMatch}
        className={STEP_CLASS}
      >
        ▼
      </PlainButton>
      <PlainButton
        aria-label="Close Find"
        title="Close Find"
        onPress={onClose}
        className={STEP_CLASS}
      >
        ✕
      </PlainButton>
    </div>
  );
}

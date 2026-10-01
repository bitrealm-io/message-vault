import Button from "../../components/Button";

/**
 * Find in conversation. The term runs on the server (`GET /v1/messages` with
 * `in:#id`), so `matchCount` is every match in the conversation, or in the
 * chosen year, and the thread below shows the matches a page at a time.
 */
export default function MessageFindBar({
  findTerm,
  onFindTermChange,
  matchCount,
  matchPosition,
  activeYear,
  onPrevMatch,
  onNextMatch,
}: {
  findTerm: string;
  onFindTermChange: (value: string) => void;
  /** Matches in the whole conversation (or year), from the server. */
  matchCount: number;
  /** Zero-based position of the highlighted match among all matches. */
  matchPosition: number;
  activeYear: number | null;
  onPrevMatch: () => void;
  onNextMatch: () => void;
}) {
  return (
    <div className="flex items-center gap-2 border-b border-border px-6 py-1.5">
      <input
        type="text"
        value={findTerm}
        onChange={(e) => onFindTermChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            if (matchCount > 0) onNextMatch();
          }
        }}
        placeholder="Find in conversation…"
        className="box-border flex-1 rounded border border-border bg-bg px-2 py-1 text-[0.813rem] text-text"
      />
      {matchCount > 0 && (
        <>
          <span className="whitespace-nowrap text-[0.75rem] text-muted">
            {matchPosition + 1} of {matchCount}
            {activeYear === null ? " in this conversation" : ` in ${activeYear}`}
          </span>
          <Button onClick={onPrevMatch} className="!px-1.5 !py-1 !text-[0.813rem]">
            ↑
          </Button>
          <Button onClick={onNextMatch} className="!px-1.5 !py-1 !text-[0.813rem]">
            ↓
          </Button>
        </>
      )}
    </div>
  );
}

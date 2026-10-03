import { type Key, ToggleButton, ToggleButtonGroup } from "react-aria-components";
import { useNavigate, useSearchParams } from "react-router-dom";
import { messageSortParam } from "../lib/messageSearchSort";
import {
  AT_PARAM,
  MESSAGE_SORT_PARAM,
  messagesSearch,
  openedAt,
  pickedMessageSort,
  type ResultsView,
  resultsView,
  VIEW_PARAM,
} from "../lib/resultsView";
import type { Conversation } from "../lib/types";
import ConversationList from "../screens/ConversationList";
import MessageSearchList from "../screens/MessageSearchList";
import { LIST_TOOLBAR_CLASS } from "./ListRangeHeader";

const SWITCH_BUTTON =
  "flex-1 cursor-pointer border-none bg-transparent px-2 py-0.5 text-[0.75rem] font-medium text-muted outline-none hover:text-text focus-visible:ring-2 focus-visible:ring-accent data-[selected]:bg-accent data-[selected]:text-sent-text";

/** The Conversations / Messages switch at the top of the Messages screen's results. */
export function ResultsViewSwitch({
  view,
  onChange,
}: {
  view: ResultsView;
  onChange: (view: ResultsView) => void;
}) {
  return (
    // The toolbar's height, so the switch lines up with the sidebar's spacer
    // and the conversation panel's toolbar beside it.
    <div className={LIST_TOOLBAR_CLASS}>
      <ToggleButtonGroup
        aria-label="Show results as"
        selectionMode="single"
        disallowEmptySelection
        selectedKeys={[view]}
        onSelectionChange={(keys: Set<Key>) => {
          const next = [...keys][0];
          if (next === "conversations" || next === "messages") onChange(next);
        }}
        className="flex flex-1 overflow-hidden rounded-md border border-border bg-elevated"
      >
        <ToggleButton id="conversations" className={SWITCH_BUTTON}>
          Conversations
        </ToggleButton>
        <ToggleButton id="messages" className={SWITCH_BUTTON}>
          Messages
        </ToggleButton>
      </ToggleButtonGroup>
    </div>
  );
}

/**
 * The Messages screen's results: the switch, then the conversations the
 * search matches or the messages it matches (#313). The header search drives
 * both. Which one, the sort picked in the Messages list, and the message a
 * result opened at all ride in the address, so opening a result keeps the
 * list as it was.
 */
export default function ResultsColumn({
  query,
  selectedConversationId,
  onSelectConversation,
}: {
  /** The search both lists run: the typed search, with a page's tag or filter in it. */
  query: string;
  selectedConversationId: number | null;
  onSelectConversation: (conversation: Conversation) => void;
}) {
  const [searchParams, setSearchParams] = useSearchParams();
  const navigate = useNavigate();
  const view = resultsView(searchParams);

  const setParam = (key: string, value: string) => {
    const next = new URLSearchParams(searchParams);
    if (value) next.set(key, value);
    else next.delete(key);
    setSearchParams(next, { replace: true });
  };

  return (
    <>
      <ResultsViewSwitch
        view={view}
        onChange={(next) => setParam(VIEW_PARAM, next === "messages" ? "messages" : "")}
      />
      {view === "messages" ? (
        <MessageSearchList
          query={query}
          sortPick={pickedMessageSort(searchParams)}
          onSortPick={(next) => setParam(MESSAGE_SORT_PARAM, messageSortParam(next))}
          selectedId={openedAt(searchParams)}
          onSelect={(message) =>
            // The conversation route lists by `q` alone, so the list's whole
            // query, a tag page's tag included, goes into it.
            navigate(
              `/messages/${message.conversation.id}${messagesSearch(searchParams, {
                q: query,
                [AT_PARAM]: String(message.id),
              })}`,
            )
          }
        />
      ) : (
        <ConversationList
          selectedId={selectedConversationId}
          onSelect={onSelectConversation}
          query={query}
        />
      )}
    </>
  );
}

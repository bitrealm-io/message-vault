import { Link, useLocation, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { apiErrorMessage } from "../lib/apiErrorMessage";
import { asMessagesLocationState } from "../lib/messagesLocationState";
import { keys } from "../lib/queryKeys";
import { AT_PARAM, openedAt } from "../lib/resultsView";
import { useRouteQuery } from "../lib/routeQuery";
import { getConversation } from "../lib/serverApi";
import MessageView from "../screens/MessageView";
import ListColumn from "./ListColumn";
import ResultsColumn from "./ResultsColumn";
import RightPane from "./RightPane";

/** The route's `:conversationId` as a number, or null when it is not a positive integer. */
function positiveInteger(raw: string | undefined): number | null {
  if (raw === undefined || !/^\d+$/.test(raw)) return null;
  const n = Number(raw);
  return Number.isSafeInteger(n) && n > 0 ? n : null;
}

export default function MessageRoute() {
  const { conversationId: conversationParam } = useParams<{ conversationId: string }>();
  const conversationId = positiveInteger(conversationParam);
  // A param was given but isn't a positive integer (e.g. "/messages/abc"), as
  // opposed to no id at all — the two render different panes below.
  const malformedId = conversationParam !== undefined && conversationId === null;
  const location = useLocation();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();

  const conversationSearch = searchParams.get("q") || "";
  const conversationFilter = searchParams.get("f") || "";
  const query = conversationFilter || conversationSearch;
  // A result in the Messages list opens its conversation at the message.
  const at = openedAt(searchParams);

  const locationState = asMessagesLocationState(location.state);
  // The router hands us whatever row the person clicked, which can be
  // arbitrarily stale (a name from before a rename, a count from before an
  // import). It seeds the first paint as `placeholderData`, never the source
  // of truth, so the fetch below still runs and replaces it.
  const stateConversation = locationState?.conversation ?? null;
  const openContactId = locationState?.openContactId ?? null;
  const openContactPreview = locationState?.openContactPreview ?? null;

  // Detail queries are keyed by number; when there's no valid id the query is
  // disabled below, so this placeholder id is never used to fetch or cache.
  const detailId = conversationId ?? 0;

  const {
    data: conversation,
    isLoading,
    error,
  } = useRouteQuery(
    keys.conversations.detail(detailId),
    (signal) => getConversation(detailId, { signal }),
    {
      enabled: conversationId !== null,
      placeholderData: stateConversation ?? undefined,
    },
  );

  const notFound = malformedId || error !== null;

  return (
    <>
      <ListColumn>
        <ResultsColumn
          query={query}
          selectedConversationId={conversationId}
          onSelectConversation={(c) => {
            // The list is filtered by this location's `q` and `f`, so the
            // conversation opened keeps them and the list stays as it was.
            // The message a result opened at belongs to the last one.
            const params = new URLSearchParams(searchParams);
            params.delete(AT_PARAM);
            const search = params.toString();
            navigate(`/messages/${c.id}${search ? `?${search}` : ""}`, {
              state: { conversation: c, openContactId, openContactPreview },
            });
          }}
        />
      </ListColumn>
      <RightPane>
        <main className="min-h-0 min-w-0 flex-1 overflow-auto bg-bg text-text">
          {conversation ? (
            // The row clicked in the list stands in as placeholder data, so
            // the pane never empties between two conversations. Keyed by id,
            // the thread starts again for each one: its page, year and find,
            // and any Move to trash or Contact Group still answering for the
            // last one, which then acts on nothing. Another result in the
            // same conversation starts it again at that message.
            <MessageView
              key={`${conversation.id}:${at ?? ""}`}
              conversation={conversation}
              openAt={at}
              onOpenContact={(contactId, preview) => {
                navigate(location.pathname + location.search, {
                  state: {
                    conversation,
                    openContactId: contactId,
                    openContactPreview: preview,
                  },
                });
              }}
            />
          ) : isLoading ? (
            <div className="flex h-full items-center justify-center text-[0.875rem] text-muted">
              Loading conversation…
            </div>
          ) : notFound ? (
            <div className="flex h-full flex-col items-center justify-center gap-3 px-6 text-center">
              <p className="m-0 text-[0.875rem] text-danger">
                {apiErrorMessage(error, "Conversation not found.")}
              </p>
              <Link
                to="/"
                className="text-[0.875rem] text-accent underline-offset-2 hover:underline"
              >
                Back to conversations
              </Link>
            </div>
          ) : (
            <div className="flex h-full items-center justify-center text-[0.875rem] text-muted">
              Select a conversation to view messages
            </div>
          )}
        </main>
      </RightPane>
    </>
  );
}

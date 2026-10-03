import { useCallback, useState } from "react";
import { Outlet, useLocation, useNavigate, useSearchParams } from "react-router-dom";
import { contactBrowseQuery } from "../lib/contactBrowseQuery";
import { groupFromSlug, slugFromPath, slugPath } from "../lib/contactGroups";
import { asMessagesLocationState } from "../lib/messagesLocationState";
import { tagFromSlug, tagListQuery } from "../lib/messageTags";
import { MESSAGE_SORT_PARAM, messagesSearch, resultsView, VIEW_PARAM } from "../lib/resultsView";
import { trashed } from "../lib/searchQuery";
import type { Conversation } from "../lib/types";
import { useContactGroups } from "../lib/useContactGroups";
import { useMessageTags } from "../lib/useMessageTags";
import ContactList from "../screens/ContactList";
import ConversationList from "../screens/ConversationList";
import AppHeader from "./AppHeader";
import CheckedContactsPanel from "./CheckedContactsPanel";
import { ColumnResizeProvider } from "./ColumnResizeContext";
import ContactDrawer from "./ContactDrawer";
import {
  type ContactBrowseKind,
  type ContactListPreviewSource,
  type ContactPreview,
  contactPreviewFromListRow,
  sameContactPreviews,
} from "./contactDrawer/contactDrawerTypes";
import LeftPanel from "./LeftPanel";
import ListColumn from "./ListColumn";
import ResultsColumn from "./ResultsColumn";
import RightPane from "./RightPane";
import { RightToolbarProvider } from "./RightToolbarContext";

type ColumnMode = "conversations" | "contacts" | "trash" | "import" | "export" | "settings";

/** Which left-column list to show for this URL. */
function modeFromPathname(pathname: string): ColumnMode {
  if (pathname.startsWith("/messages/")) return "conversations";
  if (
    pathname === "/contacts" ||
    pathname === "/no-group" ||
    pathname === "/unknown" ||
    pathname.startsWith("/group/")
  ) {
    return "contacts";
  }
  if (pathname === "/no-tag" || pathname.startsWith("/tag/")) {
    return "conversations";
  }
  if (pathname === "/trash") return "trash";
  if (pathname === "/import") return "import";
  if (pathname === "/export") return "export";
  if (pathname === "/settings") return "settings";
  return "conversations";
}

/**
 * Why a Contact Group or Message Tag page has no list: the sets have not
 * loaded yet, or no set has the name in the link. A page in either state
 * lists nothing, so it never shows every contact or conversation under the
 * set's name, and nothing on it exports them.
 */
type SetPageState = "loading" | "missing" | null;

function setPageState(slug: string | null, loading: boolean, found: string | null): SetPageState {
  if (slug === null) return null;
  if (loading) return "loading";
  return found === null ? "missing" : null;
}

/** What the list column says in place of a list for a set page it cannot show. */
function SetPageStatus({
  state,
  setLabel,
  setsLabel,
  name,
}: {
  state: "loading" | "missing";
  setLabel: string;
  setsLabel: string;
  name: string;
}) {
  return (
    <div role="status" className="p-4 text-[0.813rem] text-muted">
      {state === "loading" ? `Loading ${setsLabel}…` : `There is no ${setLabel} named ${name}.`}
    </div>
  );
}

/** Scrollable content column that hosts the routed screen. */
const mainPane = "min-w-0 flex-1 overflow-auto bg-bg text-text";

/** Centered placeholder when a column has nothing selected yet. */
const emptyMain = "flex h-full items-center justify-center text-[0.875rem] text-muted";

export default function AppLayout() {
  const location = useLocation();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();

  const [selectedContact, setSelectedContact] = useState<ContactPreview | null>(null);
  const [checkedContacts, setCheckedContacts] = useState<ContactPreview[]>([]);
  const [clearCheckedRev, setClearCheckedRev] = useState(0);
  const handleCheckedContacts = useCallback((contacts: ContactListPreviewSource[]) => {
    // The child re-maps its checked rows on every render, so store only when the
    // value actually changed — otherwise each store schedules the next render.
    setCheckedContacts((prev) => {
      const next = contacts.map(contactPreviewFromListRow);
      return sameContactPreviews(prev, next) ? prev : next;
    });
  }, []);
  const clearCheckedContacts = useCallback(() => {
    setClearCheckedRev((n) => n + 1);
  }, []);
  const { groups, loading: groupsLoading } = useContactGroups();
  const { tags, loading: tagsLoading } = useMessageTags();

  const pathname = location.pathname;
  const mode = modeFromPathname(pathname);
  const isMessageRoute = pathname.startsWith("/messages/");
  const contactsMode = mode === "contacts";
  const noGroupMode = pathname === "/no-group";
  const unknownMode = pathname === "/unknown";
  const groupSlugParam = slugFromPath(pathname, "/group");
  const activeGroup = groupSlugParam ? groupFromSlug(groupSlugParam, groups) : null;
  const groupPage = setPageState(groupSlugParam, groupsLoading, activeGroup);
  // "unknown" reaches the server as `group:unknown`, which it answers from
  // contact state rather than from stored membership.
  const groupFilter = unknownMode ? "unknown" : noGroupMode ? "none" : activeGroup;
  const noTagMode = pathname === "/no-tag";
  const tagSlugParam = slugFromPath(pathname, "/tag");
  const activeTag = tagSlugParam ? tagFromSlug(tagSlugParam, tags) : null;
  const tagPage = setPageState(tagSlugParam, tagsLoading, activeTag);
  const tagFilter = noTagMode ? "none" : activeTag;

  const conversationSearch = searchParams.get("q") || "";
  const conversationFilter = searchParams.get("f") || "";
  const contactSearch = searchParams.get("cq") || "";
  // Trash keeps its own term so leaving and returning to Trash does not inherit
  // whatever the inbox was last searching for.
  const trashSearch = searchParams.get("tq") || "";
  // Clicking a trashed row sets this instead of navigating to /messages/:id, so
  // TrashScreen can offer Restore for it without leaving the Trash list behind.
  const trashSelectedRaw = searchParams.get("tsel");
  const trashSelectedId =
    trashSelectedRaw && /^\d+$/.test(trashSelectedRaw) ? Number(trashSelectedRaw) : null;

  const trashMode = mode === "trash";
  const isFullScreen = mode === "import" || mode === "export" || mode === "settings";
  // The full-screen routes have no list to search. Export carries `?q=` for
  // its own scope box, which is not a header search.
  const searchQuery = isFullScreen
    ? ""
    : trashMode
      ? trashSearch
      : contactsMode
        ? contactSearch
        : conversationSearch;

  // `replace: true` is inherited from every other caller here and is
  // deliberate: typing in a search box must not fill the history with one
  // entry per keystroke. Trash's `tsel` selection goes through the same
  // function and so is not undoable with Back, unlike selecting a
  // conversation elsewhere, which navigates.
  function updateSearchParams(updates: Record<string, string>) {
    const next = new URLSearchParams(searchParams);
    for (const [k, v] of Object.entries(updates)) {
      if (v) next.set(k, v);
      else next.delete(k);
    }
    setSearchParams(next, { replace: true });
  }

  const handleSearch = (q: string) => {
    if (trashMode) {
      navigate(`/trash${q ? `?tq=${encodeURIComponent(q)}` : ""}`);
    } else if (contactsMode) {
      const params = q ? `?cq=${encodeURIComponent(q)}` : "";
      if (noGroupMode) {
        navigate(`/no-group${params}`);
      } else if (unknownMode) {
        navigate(`/unknown${params}`);
      } else if (groupSlugParam !== null) {
        navigate(`${slugPath("/group", groupSlugParam)}${params}`);
      } else {
        navigate(`/contacts${params}`);
      }
    } else if (pathname.startsWith("/messages/")) {
      // The open conversation stays open, at the message a result opened it
      // at, and the results keep their list and sort.
      const id = pathname.split("/")[2];
      if (id) {
        navigate(`/messages/${id}${messagesSearch(searchParams, { q })}`, {
          state: location.state,
        });
        return;
      }
      navigate(`/${messagesSearch(searchParams, { q, at: "" })}`);
    } else if (noTagMode) {
      navigate(`/no-tag${messagesSearch(searchParams, { q, at: "" })}`);
    } else if (tagSlugParam !== null) {
      navigate(`${slugPath("/tag", tagSlugParam)}${messagesSearch(searchParams, { q, at: "" })}`);
    } else {
      navigate(`/${messagesSearch(searchParams, { q, at: "" })}`);
    }
  };

  const handleSearchChange = (q: string) => {
    if (trashMode) {
      // Narrowing Trash can filter the selected row out of the left column, so
      // the selection goes with the search rather than leaving the Restore
      // panel pointed at a conversation the list no longer shows.
      updateSearchParams({ tq: q, tsel: "" });
      return;
    }
    if (contactsMode) {
      updateSearchParams({ cq: q });
      return;
    }
    updateSearchParams({ q: q, f: "" });
  };

  const trashListQuery = trashed(trashSearch);

  const threadListQuery = tagListQuery(tagFilter, conversationFilter || conversationSearch);

  // The message route filters its list by `q` and `f` alone, so the
  // conversation opened carries the list's query in them. The message route's
  // path has no tag, so a tag page's tag goes into `q`.
  const handleConversationSelect = (c: Conversation) => {
    const params = new URLSearchParams();
    if (tagFilter) {
      params.set("q", threadListQuery);
    } else {
      if (conversationSearch) params.set("q", conversationSearch);
      if (conversationFilter) params.set("f", conversationFilter);
    }
    // The results view and the Messages list's picked sort stay for when the
    // person switches back.
    for (const key of [VIEW_PARAM, MESSAGE_SORT_PARAM]) {
      const value = searchParams.get(key);
      if (value) params.set(key, value);
    }
    const search = params.toString();
    navigate(`/messages/${c.id}${search ? `?${search}` : ""}`, { state: { conversation: c } });
  };

  const locationState = asMessagesLocationState(location.state);
  const openContactId = locationState?.openContactId ?? null;
  const openContactPreview = locationState?.openContactPreview ?? null;

  const closeContactDrawer = () => {
    setSelectedContact(null);
    if (!openContactId || !locationState) return;
    const { openContactId: _closed, openContactPreview: _preview, ...rest } = locationState;
    navigate(`${location.pathname}${location.search}`, {
      replace: true,
      state: Object.keys(rest).length > 0 ? rest : null,
    });
  };

  const handleBrowseContactConversations = ({
    contactId,
    kind,
    handle,
  }: {
    contactId: string;
    kind: ContactBrowseKind;
    handle?: string;
  }) => {
    const query = contactBrowseQuery(contactId, kind, handle);
    setSelectedContact(null);
    navigate(`/?q=${encodeURIComponent(query)}&f=${encodeURIComponent(query)}`);
  };

  // What Export starts from: the conversation list's query, tag filter
  // included, and nothing when the person is on contacts, Trash, or a
  // full-screen route. A tag page with no list has nothing to export.
  const browseQuery = mode === "conversations" && tagPage === null ? threadListQuery : "";

  return (
    <RightToolbarProvider>
      <div className="flex h-screen flex-col bg-bg font-sans text-text">
        <AppHeader
          searchQuery={searchQuery}
          searchTarget={
            trashMode
              ? "trash"
              : contactsMode
                ? "contacts"
                : resultsView(searchParams) === "messages"
                  ? "messages"
                  : "conversations"
          }
          onSearchChange={handleSearchChange}
          onSearch={handleSearch}
        />
        <ColumnResizeProvider>
          <div className="flex min-h-0 flex-1 overflow-hidden">
            <LeftPanel browseQuery={browseQuery} />

            {/* Conversations: render list component directly with props */}
            {mode === "conversations" && !isMessageRoute && (
              <>
                <ListColumn>
                  {tagPage ? (
                    <SetPageStatus
                      state={tagPage}
                      setLabel="Message Tag"
                      setsLabel="Message Tags"
                      name={tagSlugParam ?? ""}
                    />
                  ) : (
                    <ResultsColumn
                      query={threadListQuery}
                      selectedConversationId={null}
                      onSelectConversation={handleConversationSelect}
                    />
                  )}
                </ListColumn>
                <RightPane>
                  <main className={mainPane}>
                    <div className={emptyMain}>Select a conversation to view messages</div>
                  </main>
                </RightPane>
              </>
            )}

            {/* Contacts: render list component directly with props */}
            {mode === "contacts" && (
              <>
                <ListColumn>
                  {groupPage ? (
                    <SetPageStatus
                      state={groupPage}
                      setLabel="Contact Group"
                      setsLabel="Contact Groups"
                      name={groupSlugParam ?? ""}
                    />
                  ) : (
                    <ContactList
                      filter={contactSearch}
                      groupFilter={groupFilter}
                      selectedId={selectedContact?.id ?? null}
                      onSelect={(c) => setSelectedContact(contactPreviewFromListRow(c))}
                      onCheckedChange={handleCheckedContacts}
                      clearCheckedRev={clearCheckedRev}
                    />
                  )}
                </ListColumn>
                <RightPane>
                  {checkedContacts.length > 0 ? (
                    <CheckedContactsPanel
                      contacts={checkedContacts}
                      onClear={clearCheckedContacts}
                    />
                  ) : selectedContact ? (
                    <ContactDrawer
                      variant="docked"
                      contactId={selectedContact.id}
                      preview={selectedContact}
                      onClose={closeContactDrawer}
                      onBrowseConversations={handleBrowseContactConversations}
                    />
                  ) : (
                    <main className={mainPane}>
                      <div className={emptyMain}>Select a contact to view details</div>
                    </main>
                  )}
                </RightPane>
              </>
            )}

            {/* Trash: ListColumn shows ConversationList with trash query; main shows TrashScreen via <Outlet /> */}
            {trashMode && (
              <>
                <ListColumn>
                  <ConversationList
                    selectedId={trashSelectedId}
                    onSelect={(c) => updateSearchParams({ tsel: String(c.id) })}
                    query={trashListQuery}
                  />
                </ListColumn>
                <RightPane>
                  <main className={mainPane}>
                    <Outlet />
                  </main>
                </RightPane>
              </>
            )}

            {/* Message route: single <Outlet /> — MessageRoute renders both ListColumn + main */}
            {isMessageRoute && (
              <div className="flex min-w-0 flex-1 overflow-hidden">
                <Outlet />
              </div>
            )}

            {/* Full-screen views: no ListColumn, just main */}
            {isFullScreen && (
              <main className={mainPane}>
                <Outlet />
              </main>
            )}

            {/* Overlay contact panel (e.g. opened from a message thread). */}
            {openContactId ? (
              <ContactDrawer
                variant="overlay"
                contactId={openContactId}
                preview={openContactPreview}
                onClose={closeContactDrawer}
                onBrowseConversations={handleBrowseContactConversations}
              />
            ) : null}
          </div>
        </ColumnResizeProvider>
      </div>
    </RightToolbarProvider>
  );
}

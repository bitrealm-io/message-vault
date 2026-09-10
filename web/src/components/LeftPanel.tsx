import { type ReactNode, useEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { canUseImportExportWithProfile } from "../lib/desktopFeatures";
import { type SavedSearch, useSavedSearchActions, useSavedSearches } from "../lib/savedSearches";
import { isTauri } from "../lib/tauri-check";
import { resizeHandleGutter } from "../lib/tw";
import { useAccountProfile } from "../lib/useAccountProfile";
import { useContactGroups } from "../lib/useContactGroups";
import { useMessageTags } from "../lib/useMessageTags";
import { Z_ROW_MENU } from "../lib/zLayers";
import { useImportAttention } from "../screens/import/useImportAttention";
import ColumnResizeHandle from "./ColumnResizeHandle";
import { useReportColumnResizing } from "./columnResizeState";
import GroupsNav from "./GroupsNav";
import { EllipsisIcon, SearchIcon, TrashIcon } from "./icons";
import { LIST_TOOLBAR_CLASS } from "./ListRangeHeader";
import {
  LEFT_PANEL_DEFAULT_WIDTH,
  LEFT_PANEL_MAX_WIDTH,
  LEFT_PANEL_MIN_WIDTH,
  LEFT_PANEL_STORAGE_KEY,
  LEFT_PANEL_WIDTH_VAR,
} from "./leftPanelWidth";
import MessageTagsNav from "./MessageTagsNav";
import NavCollapsibleSection from "./NavCollapsibleSection";
import NavGlyphButton from "./NavGlyphButton";
import {
  NAV_LEADING_GLYPH_CLASS,
  NAV_LEADING_ROW_CLASS,
  NAV_NESTED_ROW_CLASS,
  navGlyphRowClass,
} from "./navSectionLayout";
import PopupMenu from "./PopupMenu";
import SavedSearchForm from "./SavedSearchForm";
import { useColumnResize } from "./useColumnResize";

function NavIcon({ children }: { children: ReactNode }) {
  return (
    <svg
      width="15"
      height="15"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      className="shrink-0"
    >
      {children}
    </svg>
  );
}

function ConversationsIcon() {
  return (
    <NavIcon>
      {/* Message bubble */}
      <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
    </NavIcon>
  );
}

function ContactsIcon() {
  return (
    <NavIcon>
      {/* Address book */}
      <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
      <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
      <circle cx="12" cy="8" r="2" />
      <path d="M9 14c0-1.1 1.3-2 3-2s3 .9 3 2" />
    </NavIcon>
  );
}

function ImportIcon() {
  return (
    <NavIcon>
      {/* Import: arrow into tray */}
      <path d="M12 3v12" />
      <path d="m8 11 4 4 4-4" />
      <path d="M4 19h16" />
    </NavIcon>
  );
}

function ExportIcon() {
  return (
    <NavIcon>
      {/* Export: arrow out of tray */}
      <path d="M12 15V3" />
      <path d="m8 7 4-4 4 4" />
      <path d="M4 19h16" />
    </NavIcon>
  );
}

/** Browse rows: same leading slot as section headings (no extra row padding). */
function browseLinkClass(active: boolean): string {
  return `${NAV_LEADING_ROW_CLASS} box-border w-full cursor-pointer rounded border-none px-0 py-1.5 text-left text-[0.875rem] text-text hover:bg-hover ${
    active ? "bg-hover font-semibold" : "bg-transparent font-normal"
  }`;
}

export default function LeftPanel({ onSearchChange }: { onSearchChange: (v: string) => void }) {
  const location = useLocation();
  const navigate = useNavigate();
  const { profile } = useAccountProfile();
  const canImport = canUseImportExportWithProfile(isTauri(), profile);
  const importAttention = useImportAttention(canImport);
  const onDraggingChange = useReportColumnResizing();
  const { width, dragging, handleHover, handleProps } = useColumnResize({
    storageKey: LEFT_PANEL_STORAGE_KEY,
    defaultWidth: LEFT_PANEL_DEFAULT_WIDTH,
    minWidth: LEFT_PANEL_MIN_WIDTH,
    maxWidth: LEFT_PANEL_MAX_WIDTH,
    onDraggingChange,
  });

  // Keep the header brand slot aligned with the nav while it resizes.
  useEffect(() => {
    document.documentElement.style.setProperty(LEFT_PANEL_WIDTH_VAR, `${width}px`);
  }, [width]);

  useEffect(() => {
    return () => {
      document.documentElement.style.removeProperty(LEFT_PANEL_WIDTH_VAR);
    };
  }, []);

  function isActive(path: string): boolean {
    if (path === "/") {
      return (
        location.pathname === "/" ||
        location.pathname.startsWith("/messages/") ||
        location.pathname.startsWith("/tag/") ||
        location.pathname === "/no-tag"
      );
    }
    return location.pathname.startsWith(path);
  }

  const { savedSearches: groups } = useSavedSearches();
  const savedSearchActions = useSavedSearchActions();
  const [showGroupForm, setShowGroupForm] = useState(false);
  const [editFor, setEditFor] = useState<SavedSearch | null>(null);
  const [menuFor, setMenuFor] = useState<number | null>(null);
  const savedSearchMenuTriggerRef = useRef<HTMLButtonElement>(null);
  const { groups: contactGroups } = useContactGroups();
  const { tags: messageTags } = useMessageTags();

  return (
    <div
      style={{ flex: `0 0 ${width}px`, width: `${width}px` }}
      className="relative flex h-full shrink-0 flex-col overflow-hidden border-r border-border bg-panel text-text"
    >
      <div className={LIST_TOOLBAR_CLASS} aria-hidden />
      <div className={`min-h-0 flex-1 overflow-auto ${resizeHandleGutter}`}>
        {/* Browse */}
        <div className="px-3 py-2">
          <button
            type="button"
            className={browseLinkClass(isActive("/"))}
            onClick={() => navigate("/")}
          >
            <span className={NAV_LEADING_GLYPH_CLASS}>
              <ConversationsIcon />
            </span>
            Messages
          </button>
          <button
            type="button"
            className={browseLinkClass(isActive("/contacts"))}
            onClick={() => navigate("/contacts")}
          >
            <span className={NAV_LEADING_GLYPH_CLASS}>
              <ContactsIcon />
            </span>
            Contacts
          </button>
          <button
            type="button"
            className={browseLinkClass(isActive("/trash"))}
            onClick={() => navigate("/trash")}
          >
            <span className={NAV_LEADING_GLYPH_CLASS}>
              <TrashIcon size={15} />
            </span>
            Trash
          </button>
        </div>

        {/* Import/Export — desktop app only */}
        {canImport && (
          <NavCollapsibleSection
            id="messages-import-export"
            title="Messages"
            headingActive={isActive("/import") || isActive("/export")}
          >
            <button
              type="button"
              onClick={() => navigate("/import")}
              className={`${navGlyphRowClass(isActive("/import"))} cursor-pointer`}
            >
              <span className={NAV_NESTED_ROW_CLASS}>
                <span className={NAV_LEADING_GLYPH_CLASS}>
                  <ImportIcon />
                </span>
                <span className="truncate">Import</span>
                {importAttention ? (
                  <span
                    className={`ml-auto shrink-0 rounded-full px-1.5 text-[0.688rem] font-semibold leading-4 ${
                      importAttention === "failed"
                        ? "bg-danger text-sent-text"
                        : "bg-accent text-sent-text"
                    }`}
                    title={
                      importAttention === "failed"
                        ? "The last import failed"
                        : "An import is waiting for your approval"
                    }
                  >
                    {importAttention === "failed" ? "Failed" : "Waiting"}
                  </span>
                ) : null}
              </span>
            </button>
            <button
              type="button"
              onClick={() => navigate("/export")}
              className={`${navGlyphRowClass(isActive("/export"))} cursor-pointer`}
            >
              <span className={NAV_NESTED_ROW_CLASS}>
                <span className={NAV_LEADING_GLYPH_CLASS}>
                  <ExportIcon />
                </span>
                <span className="truncate">Export</span>
              </span>
            </button>
          </NavCollapsibleSection>
        )}

        <GroupsNav groups={contactGroups} />

        {/* Named search queries stored in the vault. Not contact membership. */}
        <NavCollapsibleSection
          id="saved-searches"
          title="Saved Searches"
          addLabel="Create saved search"
          onAdd={() => {
            setMenuFor(null);
            setEditFor(null);
            setShowGroupForm(true);
          }}
          className="px-3 pt-3"
        >
          {groups.length === 0 ? (
            <div className={`${NAV_LEADING_ROW_CLASS} py-1.5 text-[0.813rem] text-muted`}>
              <span className={NAV_LEADING_GLYPH_CLASS} aria-hidden />
              <span>No saved searches</span>
            </div>
          ) : (
            groups.map((g) => {
              const active =
                location.pathname === "/" &&
                location.search === `?q=${encodeURIComponent(g.query)}`;
              const menuOpen = menuFor === g.id;
              return (
                <div key={g.id} className="relative w-full">
                  <div className={navGlyphRowClass(active)}>
                    <button
                      type="button"
                      onClick={() => {
                        onSearchChange(g.query);
                        navigate(`/?q=${encodeURIComponent(g.query)}`);
                      }}
                      className={`${NAV_NESTED_ROW_CLASS} cursor-pointer border-none bg-transparent p-0 text-left text-inherit`}
                    >
                      <span className={NAV_LEADING_GLYPH_CLASS}>
                        <SearchIcon size={15} />
                      </span>
                      <span className="min-w-0 truncate">{g.name}</span>
                    </button>
                    <NavGlyphButton
                      aria-label={`Saved search options for ${g.name}`}
                      aria-haspopup="menu"
                      aria-expanded={menuOpen}
                      active={menuOpen}
                      onClick={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        savedSearchMenuTriggerRef.current = e.currentTarget;
                        setMenuFor(menuOpen ? null : g.id);
                      }}
                      className={
                        active || menuOpen
                          ? ""
                          : "opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                      }
                    >
                      <EllipsisIcon size={15} />
                    </NavGlyphButton>
                  </div>
                  <PopupMenu
                    open={menuOpen}
                    onClose={() => setMenuFor(null)}
                    triggerRef={savedSearchMenuTriggerRef}
                    label={`Saved search options for ${g.name}`}
                    className={`absolute top-full right-0 mt-0.5 ${Z_ROW_MENU}`}
                    items={[
                      {
                        label: "Rename…",
                        onSelect: () => {
                          setShowGroupForm(false);
                          setEditFor(g);
                        },
                      },
                      {
                        label: "Delete",
                        onSelect: () => {
                          void savedSearchActions.remove(g.id);
                        },
                      },
                    ]}
                  />
                </div>
              );
            })
          )}
        </NavCollapsibleSection>

        <MessageTagsNav tags={messageTags} />
      </div>

      {showGroupForm ? (
        <SavedSearchForm
          onSave={(name, query) => {
            void savedSearchActions.create(name, query);
            setShowGroupForm(false);
          }}
          onCancel={() => setShowGroupForm(false)}
        />
      ) : null}
      {editFor ? (
        <SavedSearchForm
          key={editFor.id}
          initial={{ name: editFor.name, query: editFor.query }}
          onSave={(name, query) => {
            void savedSearchActions.update(editFor.id, name, query);
            setEditFor(null);
          }}
          onCancel={() => setEditFor(null)}
        />
      ) : null}

      <ColumnResizeHandle
        ariaLabel="Resize navigation panel"
        width={width}
        minWidth={LEFT_PANEL_MIN_WIDTH}
        maxWidth={LEFT_PANEL_MAX_WIDTH}
        dragging={dragging}
        handleHover={handleHover}
        handleProps={handleProps}
      />
    </div>
  );
}

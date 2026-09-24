import { startTransition, useCallback, useEffect, useMemo, useRef, useState } from "react";
import Checkbox from "../components/Checkbox";
import ContactInitialCircle from "../components/ContactInitialCircle";
import ContactLabel from "../components/ContactLabel";
import ContactSortMenu from "../components/ContactSortMenu";
import GroupsMenu from "../components/GroupsMenu";
import InfiniteOffsetList from "../components/InfiniteOffsetList";
import { useSetRightToolbar } from "../components/useRightToolbar";
import { apiErrorMessage } from "../lib/apiErrorMessage";
import {
  contactBelongsToGroup,
  groupListQuery,
  useContactGroupActions,
  useSetContactGroupMembers,
} from "../lib/contactGroups";
import { contactLabelText } from "../lib/contactLabel";
import {
  type ContactSortState,
  compareContacts,
  contactSortLetter,
  isNameSort,
  loadContactSort,
  saveContactSort,
} from "../lib/contactSort";
import { formatDay } from "../lib/formatDate";
import { highlightText } from "../lib/highlightText";
import { PAGE_SIZE_CONTACTS_FIRST, PAGE_SIZE_FIRST } from "../lib/listPaging";
import { checksFromMembers } from "../lib/membershipChecks";
import { applyCheckedRange } from "../lib/rangeCheck";
import { hasFieldToken, stripFieldTokens } from "../lib/searchFields";
import { useTimeZone } from "../lib/timeZone";
import { UNKNOWN_GROUP } from "../lib/unknownGroup";
import { useContactGroups } from "../lib/useContactGroups";
import { listContacts } from "../lib/vaultApi";
import type { components } from "../lib/vaultApi.types";
import { keys } from "../lib/vaultKeys";
import { type PagedFetchPage, useVaultPagedList } from "../lib/vaultQuery";

const FILTER_DEBOUNCE_MS = 300;
/** Fixed row height keeps virtualization slots aligned with flex-centered content. */
const CONTACT_ROW_HEIGHT = 49;

/** The contact row as the API sends it: `id` is a real integer. */
type ContactSummary = components["schemas"]["ContactSummary"];

/**
 * A contact row as this screen keeps it. `id` is a string here because rows
 * key React lists, feed `Set<string>` selection state, and build URL-ish
 * search fragments (`with:#${id}`) — every other field is the API's own.
 */
type Contact = Omit<ContactSummary, "id"> & { id: string };

type FilterNeedles = { text: string; handle: string | null };

/** Pull plain name text and a handle:"…" value out of the filter for local matching. */
function filterNeedles(raw: string): FilterNeedles {
  const q = raw.trim();
  if (!q) return { text: "", handle: null };

  let handle: string | null = null;
  const found = q.match(/(^|\s)handle:("([^"]+)"|(\S+))/i);
  if (found) handle = found[3] ?? found[4].replace(/^"|"$/g, "");

  return { text: stripFieldTokens(q), handle };
}

/** True when this handle contains the search text. */
function handleMatchesNeedle(handle: string, needle: string): boolean {
  const n = needle.trim().toLowerCase();
  if (!n) return false;
  return handle.toLowerCase().includes(n);
}

/** Handles on this contact that match the current filter. */
function matchingHandles(addresses: string[] | undefined, filter: string): string[] {
  const { text, handle } = filterNeedles(filter);
  if (!text && !handle) return [];
  return (addresses ?? []).filter((h) => {
    if (handle && handleMatchesNeedle(h, handle)) return true;
    if (text && handleMatchesNeedle(h, text)) return true;
    return false;
  });
}

/** Text to highlight in handle subtitles. */
function highlightNeedle(filter: string): string {
  const { text, handle } = filterNeedles(filter);
  return handle || text;
}

/** True when this contact matches the typed filter, so the list can shrink as the user types. */
function contactMatchesFilter(c: Contact, filter: string): boolean {
  const { text, handle } = filterNeedles(filter);
  if (!text && !handle) return true;
  if (text && c.name.toLowerCase().includes(text.toLowerCase())) return true;
  return matchingHandles(c.addresses, filter).length > 0;
}

/** Make every contact id a string so list keys stay stable. */
function normalizeContacts(rows: ContactSummary[]): Contact[] {
  return rows.map((c) => ({
    ...c,
    id: String(c.id),
    addresses: c.addresses ?? [],
    groups: c.groups ?? [],
  }));
}

export default function ContactList({
  filter = "",
  groupFilter = null,
  selectedId = null,
  onSelect,
  onCheckedChange,
  clearCheckedRev = 0,
}: {
  filter?: string;
  /** Named group, or `"none"` for contacts with no group. */
  groupFilter?: string | "none" | null;
  selectedId?: string | null;
  onSelect: (contact: Contact) => void;
  /** Checked rows, so the right panel can list them. */
  onCheckedChange?: (contacts: Contact[]) => void;
  /** Increment to uncheck every row (Clear contacts on the selection card). */
  clearCheckedRev?: number;
}) {
  const [serverQ, setServerQ] = useState("");
  const [sortState, setSortState] = useState<ContactSortState>(() => loadContactSort());
  const zone = useTimeZone();
  const [checkedIds, setCheckedIds] = useState<Set<string>>(() => new Set());
  const [groupsMenuOpen, setGroupsMenuOpen] = useState(false);
  /** Last contacts the Groups menu assigned to, so a list filter change does not disable an open menu. */
  const assignTargetsRef = useRef<Contact[]>([]);
  /** Ignores the row click that follows a checkbox press (nested control). */
  const skipRowSelectRef = useRef(false);
  /** The last row checked or unchecked by hand: where a Shift + click range starts. */
  const rangeAnchorRef = useRef<string | null>(null);
  const catalogCompleteRef = useRef(false);
  /** Unfiltered contact list, so group clicks can filter in the browser. */
  const fullCatalogRef = useRef<Contact[] | null>(null);
  const { groups: allGroups } = useContactGroups();
  const groupActions = useContactGroupActions();
  const setGroupMembers = useSetContactGroupMembers();
  const setRightToolbar = useSetRightToolbar();

  const onSortChange = (next: ContactSortState) => {
    setSortState(next);
    saveContactSort(next);
  };

  const fetchPage = useCallback<PagedFetchPage<Contact>>(
    async ({ limit, offset, signal }) => {
      const res = await listContacts({ q: serverQ, limit, offset }, { signal });
      return {
        items: normalizeContacts(res.items),
        total: res.total,
      };
    },
    [serverQ],
  );

  const {
    items: contacts,
    total,
    loading,
    refreshing,
    filling,
    error,
    hasMore,
    loadMore: requestMore,
  } = useVaultPagedList(keys.contacts.list(serverQ), fetchPage, {
    firstPageSize: serverQ.trim() ? PAGE_SIZE_FIRST : PAGE_SIZE_CONTACTS_FIRST,
  });

  const catalogComplete =
    !loading && !refreshing && contacts.length >= total && (total > 0 || contacts.length === 0);
  catalogCompleteRef.current = catalogComplete && !serverQ.trim();
  if (catalogCompleteRef.current) {
    fullCatalogRef.current = contacts;
  }

  const groupActive = Boolean(groupFilter);
  const advancedActive = hasFieldToken(filter);

  useEffect(() => {
    void filter;
    void groupFilter;
    rangeAnchorRef.current = null;
    setCheckedIds(new Set());
  }, [filter, groupFilter]);

  useEffect(() => {
    if (clearCheckedRev === 0) return;
    rangeAnchorRef.current = null;
    setCheckedIds(new Set());
  }, [clearCheckedRev]);

  useEffect(() => {
    void catalogComplete;
    const combined = groupListQuery(groupFilter, filter);
    // Empty filter: load the full catalog.
    if (!combined.trim()) {
      setServerQ("");
      return;
    }
    // The full catalog is already in memory, so filter it in the browser.
    if (fullCatalogRef.current && !advancedActive) {
      setServerQ("");
      return;
    }
    // Group-page click: do not wait for the search debounce.
    if (groupActive && !filter.trim()) {
      setServerQ(combined);
      return;
    }
    if (catalogCompleteRef.current && !advancedActive) return;

    const t = window.setTimeout(() => setServerQ(combined), FILTER_DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [filter, catalogComplete, advancedActive, groupFilter, groupActive]);

  const filterActive = filter.trim().length > 0;
  const needles = filterNeedles(filter);
  /** Prefer plain text for names. Fall back to the handle when that is all the user typed. */
  const nameMarkTerm = needles.text || needles.handle || "";
  const handleMarkTerm = highlightNeedle(filter);

  // Filter by name and handle in the browser. Server results are used when the
  // filter has search words the client cannot apply.
  // Memoized: a fresh array here would invalidate `displayContacts` and
  // `checkedContacts` on every render, and the `onCheckedChange` effect below
  // would then re-render the parent in a loop.
  const filteredContacts = useMemo(
    () =>
      filterActive && !advancedActive
        ? contacts.filter((c) => contactMatchesFilter(c, filter))
        : contacts,
    [contacts, filter, filterActive, advancedActive],
  );

  const displayContacts = useMemo(
    () =>
      [...filteredContacts]
        .filter((c) => contactBelongsToGroup(c.groups, groupFilter))
        .sort((a, b) => compareContacts(a, b, sortState)),
    [filteredContacts, sortState, groupFilter],
  );

  const selectedContact = displayContacts.find((c) => c.id === selectedId) ?? null;
  const checkedContacts = useMemo(
    () => displayContacts.filter((c) => checkedIds.has(c.id)),
    [checkedIds, displayContacts],
  );
  const selectAllChecked =
    displayContacts.length > 0 && displayContacts.every((c) => checkedIds.has(c.id));
  const selectAllIndeterminate =
    !selectAllChecked && displayContacts.some((c) => checkedIds.has(c.id));
  const targetContacts = useMemo(() => {
    if (checkedContacts.length > 0) return checkedContacts;
    return selectedContact ? [selectedContact] : [];
  }, [checkedContacts, selectedContact]);
  if (targetContacts.length > 0) {
    assignTargetsRef.current = targetContacts;
  }
  // The ref holds rows from the last time the Groups menu was opened, so a
  // filter change that empties `targetContacts` doesn't yank the menu out
  // from under the user. Resolve those rows against the live `contacts` list
  // rather than the stale rows themselves, so a group toggle on one of them
  // (which patches `contacts`, not the ref) is reflected immediately instead
  // of the menu showing a membership that was just unchecked. Memoized on
  // `targetContacts` and `contacts` — both are themselves stable across a
  // render that changes neither — so this does not hand the effect below a
  // fresh array (and therefore a fresh `groupChecks`) on every render, which
  // would otherwise put it in the same re-render loop issue #295 fixed for
  // the tag menu.
  const assignTargets = useMemo(
    () =>
      targetContacts.length > 0
        ? targetContacts
        : assignTargetsRef.current
            .map((c) => contacts.find((x) => x.id === c.id) ?? null)
            .filter((c): c is Contact => c !== null),
    [targetContacts, contacts],
  );

  useEffect(() => {
    onCheckedChange?.(checkedContacts);
  }, [checkedContacts, onCheckedChange]);

  useEffect(() => {
    return () => onCheckedChange?.([]);
  }, [onCheckedChange]);

  const toggleChecked = (id: string) => {
    rangeAnchorRef.current = id;
    setCheckedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };
  /** Shift + click: every row from the last clicked one to this one takes this box's new state. */
  const setRangeChecked = (id: string, on: boolean) => {
    const anchor = rangeAnchorRef.current;
    rangeAnchorRef.current = id;
    setCheckedIds((prev) =>
      applyCheckedRange(
        displayContacts.map((c) => c.id),
        prev,
        anchor,
        id,
        on,
      ),
    );
  };
  const groupChecks = useMemo(
    () =>
      checksFromMembers(
        allGroups,
        assignTargets.map((c) => c.groups ?? []),
      ),
    [allGroups, assignTargets],
  );

  const applyMembership = useCallback(
    (name: string, enable: boolean) => {
      const ids = assignTargetsRef.current
        .map((c) => Number(c.id))
        .filter((id) => Number.isFinite(id) && id > 0);
      if (ids.length === 0) return Promise.resolve();
      // A refused write puts the chips back in the mutation's onError, and the
      // chips going back is the report, so there is nothing to handle here.
      return setGroupMembers
        .mutateAsync({ name, patch: enable ? { add: ids } : { remove: ids } })
        .then(
          () => undefined,
          () => undefined,
        );
    },
    [setGroupMembers.mutateAsync],
  );

  /** Drop every group on the selected contacts: one write per name, each with its own rollback. */
  const clearAllMembership = useCallback(async () => {
    const targets = assignTargetsRef.current;
    const ids = targets.map((c) => Number(c.id)).filter((id) => Number.isFinite(id) && id > 0);
    if (ids.length === 0) return;
    const names = new Set<string>();
    for (const c of targets) {
      for (const g of c.groups ?? []) names.add(g);
    }
    if (names.size === 0) return;
    await Promise.allSettled(
      [...names].map((name) => setGroupMembers.mutateAsync({ name, patch: { remove: ids } })),
    );
  }, [setGroupMembers.mutateAsync]);

  const menuDisabled = assignTargets.length === 0 && !groupsMenuOpen;

  useEffect(() => {
    setRightToolbar(
      <GroupsMenu
        allGroups={allGroups}
        checks={groupChecks}
        open={groupsMenuOpen}
        onOpenChange={setGroupsMenuOpen}
        disabled={menuDisabled}
        checksDisabled={menuDisabled}
        onToggle={(name) => {
          const on = groupChecks[name] === "on";
          void applyMembership(name, !on);
        }}
        onCreate={(name) => {
          void (async () => {
            const existing = allGroups.find((g) => g.toLowerCase() === name.toLowerCase());
            if (!existing) {
              await groupActions.create(name);
            }
            await applyMembership(existing ?? name, true);
          })();
        }}
        onClearAll={() => {
          void clearAllMembership();
        }}
      />,
    );
  }, [
    allGroups,
    applyMembership,
    clearAllMembership,
    groupChecks,
    groupsMenuOpen,
    menuDisabled,
    setRightToolbar,
    groupActions.create,
  ]);

  useEffect(() => () => setRightToolbar(null), [setRightToolbar]);

  // A–Z sections only make sense over a name; a list ordered by date has none.
  const nameSort = sortState.sort;
  const sectionLetter = isNameSort(nameSort)
    ? (c: Contact) => contactSortLetter(contactLabelText(c.name, c.addresses), nameSort)
    : undefined;

  const localSlice =
    !advancedActive &&
    (catalogCompleteRef.current || !serverQ.trim()) &&
    (filterActive || groupActive);
  const rangeTotal = localSlice ? displayContacts.length : total;

  return (
    <InfiniteOffsetList
      items={displayContacts}
      total={total}
      rangeTotal={rangeTotal}
      loading={loading}
      refreshing={refreshing}
      filling={filling}
      error={error ? apiErrorMessage(error, "Could not load contacts.") : ""}
      hasMore={hasMore}
      requestMore={requestMore}
      estimateSize={CONTACT_ROW_HEIGHT}
      dynamicSize={filterActive}
      selectedId={selectedId}
      onSelect={(c) => {
        if (skipRowSelectRef.current) {
          skipRowSelectRef.current = false;
          return;
        }
        if (checkedIds.size > 0) {
          toggleChecked(c.id);
          return;
        }
        onSelect(c);
      }}
      isRowHighlighted={(c) => (checkedIds.size > 0 ? checkedIds.has(c.id) : c.id === selectedId)}
      selectAllChecked={selectAllChecked}
      selectAllIndeterminate={selectAllIndeterminate}
      onSelectAllChange={(on) => {
        rangeAnchorRef.current = null;
        startTransition(() => {
          setCheckedIds(on ? new Set(displayContacts.map((c) => c.id)) : new Set());
        });
      }}
      selectAllLabel="Select all contacts"
      getId={(c) => c.id}
      getTextValue={(c) => contactLabelText(c.name, c.addresses)}
      ariaLabel="Contacts"
      errorPrefix="Could not load contacts"
      headerActions={<ContactSortMenu state={sortState} onChange={onSortChange} />}
      getSectionLetter={filterActive ? undefined : sectionLetter}
      empty={
        !loading ? (
          <div className="p-4 text-[0.813rem] text-muted">
            {filterActive
              ? "No contacts match this filter"
              : groupFilter === "none"
                ? "Every contact has a group"
                : groupFilter === UNKNOWN_GROUP
                  ? "Every contact has a name and a way to reach them"
                  : groupFilter
                    ? "No contacts in this group"
                    : "No contacts"}
          </div>
        ) : null
      }
      renderRowLead={(c) => {
        const checked = checkedIds.has(c.id);
        // The whole avatar square toggles the box, so the label points at it by id.
        const checkId = `contact-check-${c.id}`;
        return (
          <label
            htmlFor={checkId}
            className="group/avatar relative flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center self-center"
            onPointerDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              skipRowSelectRef.current = true;
              queueMicrotask(() => {
                skipRowSelectRef.current = false;
              });
            }}
            onKeyDown={(e) => e.stopPropagation()}
          >
            {/*
             * The initials hide behind the checkbox on hover, on keyboard focus,
             * and once checked. `opacity-0` rather than `invisible` so the input
             * stays in the tab order when it is not yet visible.
             */}
            <span
              className={
                checked
                  ? "invisible"
                  : "group-hover/avatar:invisible group-focus-within/avatar:invisible"
              }
            >
              <ContactInitialCircle
                displayName={c.name}
                preferredHandle={c.addresses?.[0] ?? null}
              />
            </span>
            <Checkbox
              id={checkId}
              checked={checked}
              aria-label={`Select ${contactLabelText(c.name, c.addresses)}`}
              onChange={(on, e) => {
                // A checkbox change is a click underneath, so the Shift key is on it.
                if ((e.nativeEvent as MouseEvent).shiftKey) setRangeChecked(c.id, on);
                else toggleChecked(c.id);
              }}
              className={`absolute ${
                checked ? "" : "opacity-0 group-hover/avatar:opacity-100 focus-visible:opacity-100"
              }`}
            />
          </label>
        );
      }}
      renderRow={(c) => {
        const nameKey = contactLabelText(c.name, c.addresses).toLowerCase();
        const shownHandles = filterActive
          ? matchingHandles(c.addresses, filter).filter((h) => h.trim().toLowerCase() !== nameKey)
          : [];
        return (
          <div className="min-w-0 flex-1">
            <div className="flex items-baseline justify-between gap-2">
              <div className="min-w-0 flex-1 truncate text-[0.875rem] font-medium">
                <ContactLabel
                  name={c.name}
                  addresses={c.addresses}
                  render={(text) =>
                    filterActive && nameMarkTerm ? highlightText(text, nameMarkTerm) : text
                  }
                />
              </div>
              {/* When the vault last heard from them; blank for a contact that never wrote. */}
              {c.last_heard_at ? (
                <span
                  className="shrink-0 text-[0.75rem] text-muted"
                  title="Last heard from"
                  data-testid="contact-last-heard"
                >
                  {formatDay(c.last_heard_at, zone)}
                </span>
              ) : null}
            </div>
            {shownHandles.length > 0 && (
              <div className="mt-0.5">
                {shownHandles.map((h) => (
                  <div key={h} className="truncate text-[0.75rem] text-muted">
                    {highlightText(h, handleMarkTerm)}
                  </div>
                ))}
              </div>
            )}
          </div>
        );
      }}
    />
  );
}

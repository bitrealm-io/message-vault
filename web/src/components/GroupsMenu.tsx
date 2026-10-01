import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { isReservedGroupName, reservedGroupError } from "../lib/contactGroups";
import type { MembershipCheckState } from "../lib/membershipChecks";
import { popupShadow } from "../lib/uiStyles";
import { useDismissable } from "../lib/useDismissable";
import { Z_POPOVER } from "../lib/zLayers";
import Checkbox from "./Checkbox";
import { ChevronDownIcon, PeopleGroupIcon } from "./icons";

export type GroupCheckState = MembershipCheckState;

/** Padding, type size, and flex line shared by group rows and the empty message. */
const MENU_ROW_CLASS = "flex items-center gap-2 px-3 py-1.5 text-[0.813rem] leading-5";

/** Assign or remove groups (or tags) on the selected rows. */
export default function GroupsMenu({
  allGroups,
  checks,
  onToggle,
  onCreate,
  onClearAll,
  disabled = false,
  ariaLabel = "Contact Groups",
  title = "Contact Groups",
  searchPlaceholder = "Search groups…",
  emptyText = "No groups",
  noMatchText = "No matching groups",
  createButtonLabel = "Create group",
  createTitle = "Create contact group",
  createPlaceholder = "Group name",
  isReserved = isReservedGroupName,
  reservedError = reservedGroupError,
  icon,
  /** Show "Groups" (or ariaLabel) plus the assign-groups icon. Off for icon-only tags. */
  labeled = true,
  open: openProp,
  onOpenChange,
  /** When set, checkboxes stay clickable even if the trigger is disabled. */
  checksDisabled,
}: {
  allGroups: string[];
  checks: Record<string, GroupCheckState>;
  onToggle?: (name: string) => void;
  onCreate?: (name: string) => void;
  onClearAll?: () => void;
  disabled?: boolean;
  ariaLabel?: string;
  title?: string;
  searchPlaceholder?: string;
  emptyText?: string;
  noMatchText?: string;
  createButtonLabel?: string;
  createTitle?: string;
  createPlaceholder?: string;
  isReserved?: (name: string) => boolean;
  reservedError?: (name: string) => string;
  icon?: ReactNode;
  labeled?: boolean;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  checksDisabled?: boolean;
}) {
  const [uncontrolledOpen, setUncontrolledOpen] = useState(false);
  const open = openProp ?? uncontrolledOpen;
  const setOpen = useCallback(
    (next: boolean) => {
      if (openProp === undefined) setUncontrolledOpen(next);
      onOpenChange?.(next);
    },
    [openProp, onOpenChange],
  );
  const boxesDisabled = checksDisabled ?? disabled;
  const [mode, setMode] = useState<"list" | "create">("list");
  const [query, setQuery] = useState("");
  const [newName, setNewName] = useState("");
  const [createError, setCreateError] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const nameRef = useRef<HTMLInputElement>(null);

  const dismiss = useCallback(() => {
    setOpen(false);
    setMode("list");
  }, [setOpen]);
  useDismissable(open, rootRef, dismiss);

  useEffect(() => {
    if (!open) return;
    if (mode === "list") {
      setQuery("");
      requestAnimationFrame(() => searchRef.current?.focus());
    } else {
      setNewName("");
      setCreateError(null);
      requestAnimationFrame(() => nameRef.current?.focus());
    }
  }, [open, mode]);

  const visibleGroups = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return allGroups;
    return allGroups.filter((g) => g.toLowerCase().includes(q));
  }, [allGroups, query]);

  const hasAnyMembership = Object.values(checks).some(
    (state) => state === "on" || state === "mixed",
  );
  const listEmptyText = query.trim() ? noMatchText : emptyText;
  const toneClass = open ? "text-accent" : "text-muted";
  const popoverClass = `absolute top-full left-0 mt-1 w-64 rounded-xl border border-border bg-popover ${Z_POPOVER} ${popupShadow}`;

  const saveNew = () => {
    if (disabled || !onCreate) return;
    const name = newName.trim();
    if (!name) return;
    if (isReserved(name)) {
      setCreateError(reservedError(name));
      return;
    }
    onCreate(name);
    setNewName("");
    setMode("list");
  };

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        aria-label={ariaLabel}
        aria-expanded={open}
        disabled={disabled}
        title={title}
        onClick={() => {
          if (disabled) return;
          setOpen(!open);
          setMode("list");
        }}
        className={
          labeled
            ? `inline-flex h-7 cursor-pointer items-center gap-1.5 rounded-md border border-border bg-elevated px-2.5 text-[0.75rem] font-medium hover:text-text disabled:cursor-default disabled:opacity-40 ${toneClass}`
            : `flex h-7 w-7 cursor-pointer items-center justify-center rounded-md border border-border bg-elevated hover:text-text disabled:cursor-default disabled:opacity-40 ${toneClass}`
        }
      >
        {icon ?? <PeopleGroupIcon size={16} />}
        {labeled ? <span>{title}</span> : null}
        {labeled ? (
          <ChevronDownIcon
            size={12}
            className={`shrink-0 transition-transform duration-150${open ? " rotate-180" : ""}`}
          />
        ) : null}
      </button>
      {open && mode === "list" ? (
        <div data-mc-overlay="" className={popoverClass}>
          <div className="border-b border-border p-2">
            <input
              ref={searchRef}
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={searchPlaceholder}
              aria-label={searchPlaceholder}
              className="box-border w-full rounded border border-border bg-elevated px-2 py-1.5 text-[0.813rem] text-text outline-none focus:border-accent"
            />
          </div>
          <div className="max-h-56 overflow-y-auto py-1">
            {visibleGroups.length === 0 ? (
              <div role="status" className={`${MENU_ROW_CLASS} text-muted`}>
                <span className="size-3.5 shrink-0" aria-hidden />
                <span>{listEmptyText}</span>
              </div>
            ) : (
              visibleGroups.map((name) => {
                const state = checks[name] ?? "off";
                return (
                  <Checkbox
                    key={name}
                    labelClassName={`${MENU_ROW_CLASS} w-full text-text hover:bg-hover`}
                    checked={state === "on"}
                    indeterminate={state === "mixed"}
                    disabled={boxesDisabled}
                    onChange={() => onToggle?.(name)}
                  >
                    <span className="truncate">{name}</span>
                  </Checkbox>
                );
              })
            )}
          </div>
          <div className="border-t border-border py-1">
            <button
              type="button"
              disabled={boxesDisabled}
              onClick={() => setMode("create")}
              className="flex w-full cursor-pointer items-center gap-2 border-none bg-transparent px-3 py-1.5 text-left text-[0.813rem] text-text hover:bg-hover disabled:opacity-50"
            >
              <span className="text-muted">+</span>
              {createButtonLabel}
            </button>
            {onClearAll ? (
              <button
                type="button"
                disabled={boxesDisabled || !hasAnyMembership}
                onClick={() => onClearAll()}
                className="flex w-full cursor-pointer items-center gap-2 border-none bg-transparent px-3 py-1.5 text-left text-[0.813rem] text-text hover:bg-hover disabled:opacity-50"
              >
                Clear all
              </button>
            ) : null}
          </div>
        </div>
      ) : null}
      {open && mode === "create" ? (
        <div data-mc-overlay="" className={`${popoverClass} p-3`}>
          <h3 className="text-[0.875rem] font-semibold text-text">{createTitle}</h3>
          <input
            ref={nameRef}
            type="text"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                saveNew();
              }
            }}
            placeholder={createPlaceholder}
            disabled={disabled}
            className="mt-2 box-border w-full rounded border border-border bg-elevated px-2 py-1.5 text-[0.813rem] text-text"
          />
          {createError ? <p className="mt-1 text-[0.75rem] text-danger">{createError}</p> : null}
          <div className="mt-3 flex items-center gap-2">
            <button
              type="button"
              disabled={disabled || !newName.trim()}
              onClick={saveNew}
              className="cursor-pointer rounded-md bg-accent px-3 py-1 text-[0.813rem] font-medium text-sent-text disabled:opacity-40"
            >
              Create
            </button>
            <button
              type="button"
              onClick={() => setMode("list")}
              className="cursor-pointer rounded-md bg-elevated px-3 py-1 text-[0.813rem] text-text hover:bg-hover"
            >
              Cancel
            </button>
          </div>
        </div>
      ) : null}
    </div>
  );
}

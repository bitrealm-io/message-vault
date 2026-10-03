import { useCallback, useRef, useState } from "react";
import { popupShadow } from "../lib/uiStyles";
import { useMenuKeyboard } from "../lib/useMenuKeyboard";
import { Z_POPOVER } from "../lib/zLayers";

export type SortOrder = "asc" | "desc";

export type SortField<Id extends string> = {
  id: Id;
  label: string;
};

/**
 * The sort control that sits at the right of a list header.
 *
 * The fields differ per list — contacts sort by name, conversations by date or
 * message count — but the button, its position, and the menu are the same, so
 * the lists cannot drift apart visually.
 */
export default function SortMenu<Id extends string>({
  fields,
  sort,
  order,
  onChange,
  itemNoun,
  ascLabel = "Ascending",
  descLabel = "Descending",
  unordered = [],
}: {
  fields: ReadonlyArray<SortField<Id>>;
  sort: Id;
  order: SortOrder;
  onChange: (next: { sort: Id; order: SortOrder }) => void;
  /** Plural noun for the accessible name, e.g. "contacts" or "conversations". */
  itemNoun: string;
  ascLabel?: string;
  descLabel?: string;
  /** Fields that have one order of their own, such as Relevance: no Order choice is shown for them. */
  unordered?: ReadonlyArray<Id>;
}) {
  const [open, setOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  const { onKeyDown } = useMenuKeyboard(open, menuRef, close, triggerRef);

  const sortLabel = fields.find((f) => f.id === sort)?.label ?? fields[0]?.label ?? "";
  const ordered = !unordered.includes(sort);
  const orderLabel = order === "asc" ? ascLabel : descLabel;
  const sortedBy = ordered ? `${sortLabel}, ${orderLabel}` : sortLabel;

  return (
    <div className="relative">
      <button
        type="button"
        ref={triggerRef}
        aria-label={`Sort ${itemNoun} by ${sortedBy}`}
        aria-expanded={open}
        aria-haspopup="menu"
        title={`Sorted by ${sortedBy}`}
        onClick={() => setOpen((v) => !v)}
        className="flex h-7 w-7 cursor-pointer items-center justify-center rounded-md border border-border bg-elevated text-muted hover:text-text"
      >
        <SortIcon />
      </button>
      {open ? (
        <div
          ref={menuRef}
          role="menu"
          aria-label={`Sort ${itemNoun}`}
          data-mc-overlay=""
          onKeyDown={onKeyDown}
          className={`absolute top-full right-0 mt-1 min-w-[10.5rem] rounded-xl border border-border bg-popover py-2 ${Z_POPOVER} ${popupShadow}`}
        >
          <div className="px-3 pb-1.5 text-[0.75rem] font-semibold text-text">Sort By</div>
          {fields.map((field) => (
            <SortOption
              key={field.id}
              label={field.label}
              selected={sort === field.id}
              onSelect={() => {
                onChange({ sort: field.id, order });
                setOpen(false);
              }}
            />
          ))}
          {ordered ? (
            <>
              <div className="my-1.5 border-t border-border" />
              <div className="px-3 pb-1.5 text-[0.75rem] font-semibold text-text">Order</div>
              <SortOption
                label={ascLabel}
                selected={order === "asc"}
                onSelect={() => {
                  onChange({ sort, order: "asc" });
                  setOpen(false);
                }}
              />
              <SortOption
                label={descLabel}
                selected={order === "desc"}
                onSelect={() => {
                  onChange({ sort, order: "desc" });
                  setOpen(false);
                }}
              />
            </>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function SortOption({
  label,
  selected,
  onSelect,
}: {
  label: string;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitemradio"
      aria-checked={selected}
      onClick={onSelect}
      className="flex w-full cursor-pointer items-center gap-2 border-none bg-transparent px-3 py-1.5 text-left text-[0.813rem] text-text outline-none hover:bg-hover-strong focus-visible:bg-hover-strong"
    >
      <span className="flex w-4 justify-center text-accent">{selected ? <CheckIcon /> : null}</span>
      {label}
    </button>
  );
}

function SortIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden>
      <path
        d="M5 3v10M5 3l-2.5 2.5M5 3l2.5 2.5M11 13V3M11 13l-2.5-2.5M11 13l2.5-2.5"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 12 12" fill="none" aria-hidden>
      <path
        d="M2 6.2L4.6 9 10 3"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

import { Header, Menu, MenuItem, MenuSection, MenuTrigger, Popover } from "react-aria-components";
import { menuItemClass, menuPopoverClass } from "../lib/uiStyles";
import PlainButton from "./PlainButton";

export type SortOrder = "asc" | "desc";

export type SortField<Id extends string> = {
  id: Id;
  label: string;
};

const sectionHeaderClass = "px-3 pb-1.5 text-[0.75rem] font-semibold text-text";

/**
 * The sort control that sits at the right of a list header.
 *
 * The fields differ per list — contacts sort by name, conversations by date or
 * message count — but the button, its position, and the menu are the same, so
 * the lists cannot drift apart visually.
 *
 * The menu is React Aria's, with two single-selection sections, so each choice
 * is a `menuitemradio` and picking one closes the menu.
 */
export default function SortMenu<Id extends string>({
  fields,
  sort,
  order,
  onChange,
  itemNoun,
  ascLabel = "Ascending",
  descLabel = "Descending",
}: {
  fields: ReadonlyArray<SortField<Id>>;
  sort: Id;
  order: SortOrder;
  onChange: (next: { sort: Id; order: SortOrder }) => void;
  /** Plural noun for the accessible name, e.g. "contacts" or "conversations". */
  itemNoun: string;
  ascLabel?: string;
  descLabel?: string;
}) {
  const sortLabel = fields.find((f) => f.id === sort)?.label ?? fields[0]?.label ?? "";
  const orderLabel = order === "asc" ? ascLabel : descLabel;

  return (
    <MenuTrigger>
      <PlainButton
        aria-label={`Sort ${itemNoun} by ${sortLabel}, ${orderLabel}`}
        title={`Sorted by ${sortLabel}, ${orderLabel}`}
        className="flex h-7 w-7 cursor-pointer items-center justify-center rounded-md border border-border bg-elevated text-muted outline-none hover:text-text focus-visible:ring-2 focus-visible:ring-accent aria-expanded:text-text"
      >
        <SortIcon />
      </PlainButton>
      <Popover
        placement="bottom end"
        offset={4}
        data-mc-overlay=""
        className={`${menuPopoverClass} min-w-[10.5rem] rounded-xl py-2`}
      >
        <Menu aria-label={`Sort ${itemNoun}`} shouldFocusWrap className="outline-none">
          <MenuSection
            selectionMode="single"
            disallowEmptySelection
            selectedKeys={[sort]}
            onSelectionChange={(keys) => {
              const next = fields.find((f) => keys !== "all" && keys.has(f.id));
              if (next) onChange({ sort: next.id, order });
            }}
          >
            <Header className={sectionHeaderClass}>Sort By</Header>
            {fields.map((field) => (
              <SortOption key={field.id} id={field.id} label={field.label} />
            ))}
          </MenuSection>
          <MenuSection
            selectionMode="single"
            disallowEmptySelection
            selectedKeys={[order]}
            onSelectionChange={(keys) => {
              if (keys === "all") return;
              if (keys.has("asc")) onChange({ sort, order: "asc" });
              else if (keys.has("desc")) onChange({ sort, order: "desc" });
            }}
            className="mt-1.5 block border-t border-border pt-1.5"
          >
            <Header className={sectionHeaderClass}>Order</Header>
            <SortOption id="asc" label={ascLabel} />
            <SortOption id="desc" label={descLabel} />
          </MenuSection>
        </Menu>
      </Popover>
    </MenuTrigger>
  );
}

function SortOption({ id, label }: { id: string; label: string }) {
  return (
    <MenuItem id={id} textValue={label} className={`${menuItemClass} text-text`}>
      {({ isSelected }) => (
        <>
          <span className="flex w-4 justify-center text-accent">
            {isSelected ? <CheckIcon /> : null}
          </span>
          {label}
        </>
      )}
    </MenuItem>
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

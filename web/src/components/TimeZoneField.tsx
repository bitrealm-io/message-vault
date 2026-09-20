import { useEffect, useMemo, useState } from "react";
import {
  Button,
  ComboBox,
  Header,
  Input,
  Label,
  ListBox,
  ListBoxItem,
  ListBoxSection,
  Popover,
} from "react-aria-components";
import { browserTimeZone } from "../lib/timeZone";
import { choiceForZone, searchTimeZones } from "../lib/timeZoneChoices";
import { popupShadow } from "../lib/uiStyles";
import { Z_POPOVER } from "../lib/zLayers";
import { selectItemClassName } from "./Select";
import { textInputClassName } from "./TextField";

/** The key of the row that repeats this browser's zone above the full list. */
const BROWSER_ROW = "browser-zone";

const sectionHeaderClass =
  "px-2 pt-1.5 pb-1 text-[0.688rem] font-semibold uppercase tracking-[0.05em] text-muted";

/**
 * The time zone picker, the one control Profile Setup and Settings → Profile
 * both use. Typing narrows the rows by city, country, abbreviation, offset or
 * IANA name; with nothing typed, this browser's zone sits above the full list.
 *
 * `value` is the stored IANA name and `onChange` is given the picked row's.
 * The field shows the picked row's label, and goes back to it when the person
 * leaves without picking.
 */
export default function TimeZoneField({
  value,
  onChange,
  label,
  isDisabled,
  className,
}: {
  value: string;
  onChange: (zone: string) => void;
  /** A visible label; without one the field is named "Time zone". */
  label?: string;
  isDisabled?: boolean;
  className?: string;
}) {
  const selected = useMemo(() => choiceForZone(value), [value]);
  const browser = useMemo(() => choiceForZone(browserTimeZone()), []);
  const [text, setText] = useState(selected.label);

  useEffect(() => {
    setText(selected.label);
  }, [selected.label]);

  // The picked row's own label in the field is not a search.
  const query = text === selected.label ? "" : text;
  const rows = useMemo(() => {
    const found = searchTimeZones(query);
    // A stored zone no row knows still has to be in the list to be shown picked.
    return found.some((c) => c.id === selected.id) || query ? found : [selected, ...found];
  }, [query, selected]);

  return (
    <ComboBox
      aria-label={label ? undefined : "Time zone"}
      className={className}
      isDisabled={isDisabled}
      menuTrigger="focus"
      allowsEmptyCollection
      // Given its items, the combobox leaves narrowing to this component, which
      // matches on more than the label a row shows.
      items={rows}
      selectedKey={selected.id}
      inputValue={text}
      onInputChange={setText}
      onSelectionChange={(key) => {
        // Emptying the field reports null: not a pick, the zone stays.
        if (typeof key !== "string") return;
        const zone = key === BROWSER_ROW ? browser.id : key;
        setText(choiceForZone(zone).label);
        if (zone !== selected.id) onChange(zone);
      }}
      onOpenChange={(open) => {
        if (!open) setText(selected.label);
      }}
    >
      {label && <Label className="mb-1 block text-[0.875rem] font-medium text-text">{label}</Label>}
      <div className="relative">
        <Input
          className={`${textInputClassName} pr-9`}
          placeholder="Search by city, country or zone"
          // Typing replaces the picked row's label. Focus comes back to the
          // field each time the rows change, and by then it holds a search.
          onFocus={(e) => {
            if (e.currentTarget.value === selected.label) e.currentTarget.select();
          }}
        />
        <Button className="absolute inset-y-0 right-0 flex w-9 items-center justify-center border-0 bg-transparent text-muted outline-none">
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <path
              d="M2.5 3.5 5 6l2.5-2.5"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </Button>
      </div>
      <Popover
        data-mv-overlay=""
        className={`box-border w-[var(--trigger-width)] rounded-md border border-border bg-popover p-1 outline-none ${Z_POPOVER} ${popupShadow}`}
      >
        <ListBox
          className="max-h-72 overflow-auto outline-none"
          renderEmptyState={() => (
            <div className="px-2 py-1 text-[0.875rem] text-muted">No time zone matches.</div>
          )}
        >
          {!query && (
            <ListBoxSection>
              <Header className={sectionHeaderClass}>This browser</Header>
              <ListBoxItem
                id={BROWSER_ROW}
                textValue={browser.label}
                className={selectItemClassName}
              >
                {browser.label}
              </ListBoxItem>
            </ListBoxSection>
          )}
          {/* An empty section still counts as content, and would hide the empty state. */}
          {rows.length > 0 && (
            <ListBoxSection>
              {!query && <Header className={sectionHeaderClass}>All time zones</Header>}
              {rows.map((c) => (
                <ListBoxItem
                  key={c.id}
                  id={c.id}
                  textValue={c.label}
                  className={selectItemClassName}
                >
                  {c.label}
                </ListBoxItem>
              ))}
            </ListBoxSection>
          )}
        </ListBox>
      </Popover>
    </ComboBox>
  );
}

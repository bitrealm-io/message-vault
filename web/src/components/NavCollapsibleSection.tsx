import { type ReactNode, useState } from "react";
import { readPref, writePref } from "../lib/storage";
import { ChevronRightIcon, PlusIcon } from "./icons";
import NavGlyphButton from "./NavGlyphButton";
import {
  NAV_LEADING_GLYPH_CLASS,
  NAV_LEADING_ROW_CLASS,
  NAV_SECTION_GRID_CLASS,
} from "./navSectionLayout";

const STORAGE_PREFIX = "mc-left-nav-open:";

function readOpen(id: string): boolean {
  return readPref(STORAGE_PREFIX + id) !== "0";
}

function writeOpen(id: string, open: boolean) {
  writePref(STORAGE_PREFIX + id, open ? "1" : "0");
}

/**
 * Sidebar block whose heading toggles the list.
 * When addLabel and onAdd are set, the trailing plus creates an item;
 * otherwise the 1.5rem trailing slot stays as an empty spacer so titles align,
 * and the heading button covers that slot so the whole row toggles.
 */
export default function NavCollapsibleSection({
  id,
  title,
  addLabel,
  onAdd,
  addDisabled = false,
  headingActive = false,
  className = "px-3 py-2",
  children,
}: {
  id: string;
  title: string;
  addLabel?: string;
  onAdd?: () => void;
  addDisabled?: boolean;
  /** Tint the heading when a nested route is current (e.g. Import while collapsed). */
  headingActive?: boolean;
  className?: string;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(() => readOpen(id));
  const showAdd = addLabel != null && onAdd != null;
  const headingClass = `cursor-pointer border-none bg-transparent p-0 text-left text-[0.875rem] font-bold text-text ${
    headingActive ? "rounded bg-hover" : ""
  }`;

  const toggleOpen = () => {
    setOpen((prev) => {
      const next = !prev;
      writeOpen(id, next);
      return next;
    });
  };

  const titleInner = (
    <>
      <span className={NAV_LEADING_GLYPH_CLASS}>
        <ChevronRightIcon
          size={12}
          className={`text-muted transition-transform duration-150 motion-reduce:transition-none ${
            open ? "rotate-90" : ""
          }`}
        />
      </span>
      <span className="truncate">{title}</span>
    </>
  );

  return (
    <div className={className}>
      <div className={`mb-1 ${NAV_SECTION_GRID_CLASS}`}>
        {showAdd ? (
          <button
            type="button"
            aria-expanded={open}
            onClick={toggleOpen}
            className={`${NAV_LEADING_ROW_CLASS} ${headingClass}`}
          >
            {titleInner}
          </button>
        ) : (
          <button
            type="button"
            aria-expanded={open}
            onClick={toggleOpen}
            className={`${NAV_SECTION_GRID_CLASS} col-span-2 ${headingClass}`}
          >
            <span className={NAV_LEADING_ROW_CLASS}>{titleInner}</span>
            <span aria-hidden className="size-6 shrink-0" />
          </button>
        )}
        {showAdd ? (
          <NavGlyphButton aria-label={addLabel} disabled={addDisabled} onClick={onAdd}>
            <PlusIcon size={14} />
          </NavGlyphButton>
        ) : null}
      </div>
      {open ? children : null}
    </div>
  );
}

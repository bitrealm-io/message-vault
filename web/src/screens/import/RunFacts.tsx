import { type ReactNode, useId, useState } from "react";

/**
 * The rows a stage of an Import Run shows under its label: named groups of
 * facts, one fact per line, every value on the same right-hand edge.
 */
export function FactGroups({ children }: { children: ReactNode }) {
  return <div className="mt-2 flex flex-col gap-3 text-[0.813rem]">{children}</div>;
}

/** One named group. `value` sits on the heading line when the group has a total of its own. */
export function FactGroup({
  title,
  caption,
  value,
  children,
}: {
  title: string;
  /** Muted words after the title that say what the group's rows are. */
  caption?: string;
  value?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <section>
      <div className="flex items-baseline justify-between gap-4 py-0.5">
        <h3 className="m-0 text-[0.813rem] font-semibold text-text">
          {title}
          {caption ? <span className="ml-1.5 font-normal text-muted">{caption}</span> : null}
        </h3>
        {value != null ? (
          <span className="min-w-0 text-right tabular-nums text-text [overflow-wrap:anywhere]">
            {value}
          </span>
        ) : null}
      </div>
      {children}
    </section>
  );
}

/** One fact inside a group: a muted label and its value. */
export function FactRow({
  label,
  value,
  labelIsValue,
}: {
  label: ReactNode;
  value?: ReactNode;
  /** The label is itself the fact (an identity, a file name), so it reads as text, not as a caption. */
  labelIsValue?: boolean;
}) {
  return (
    <div className="flex items-baseline justify-between gap-4 py-0.5 pl-4">
      <span
        className={`min-w-0 [overflow-wrap:anywhere] ${labelIsValue ? "text-text" : "text-muted"}`}
      >
        {label}
      </span>
      {value != null ? (
        <span className="min-w-0 text-right tabular-nums text-text [overflow-wrap:anywhere]">
          {value}
        </span>
      ) : null}
    </div>
  );
}

/**
 * A fact whose count opens, in place, to the things it counts. Closed by
 * default: the count is what a person reads first, the list is there when
 * they want to check it.
 */
export function ExpandableFactRow({
  label,
  caption,
  value,
  children,
}: {
  label: string;
  /** Muted words after the label that say what becomes of the things it counts. */
  caption?: string;
  value?: ReactNode;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const panelId = useId();
  return (
    <div>
      <button
        type="button"
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => setOpen((was) => !was)}
        className="flex w-full items-baseline justify-between gap-4 rounded border-0 bg-transparent py-0.5 pl-4 pr-0 text-left text-[0.813rem] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent"
      >
        <span className="min-w-0 text-accent [overflow-wrap:anywhere]">
          <span
            aria-hidden
            className={`mr-1 inline-block transition-transform motion-reduce:transition-none ${open ? "rotate-90" : ""}`}
          >
            ▸
          </span>
          {label}
          {caption ? <span className="ml-1.5 text-muted">{caption}</span> : null}
        </span>
        {value != null ? <span className="tabular-nums text-text">{value}</span> : null}
      </button>
      {open ? (
        <div id={panelId} className="pb-1 pl-8">
          {children}
        </div>
      ) : null}
    </div>
  );
}

/** How many rows an opened list shows before "Show all". */
const LIST_PREVIEW = 5;

/**
 * The opened list under an expandable fact: a line saying what the list is,
 * then its rows, cut to the first few until the person asks for all of them.
 */
export function FactList<T>({
  note,
  items,
  itemKey,
  renderName,
  renderValue,
}: {
  note?: string;
  items: T[];
  itemKey: (item: T) => string;
  renderName: (item: T) => ReactNode;
  renderValue?: (item: T) => ReactNode;
}) {
  const [all, setAll] = useState(false);
  const shown = all ? items : items.slice(0, LIST_PREVIEW);
  return (
    <>
      {note ? <p className="m-0 mb-1 text-muted">{note}</p> : null}
      <ul className="m-0 flex max-h-64 list-none flex-col gap-0.5 overflow-y-auto p-0">
        {shown.map((item) => (
          <li key={itemKey(item)} className="flex items-baseline justify-between gap-4">
            <span className="min-w-0 text-text [overflow-wrap:anywhere]">{renderName(item)}</span>
            {renderValue ? (
              <span className="shrink-0 whitespace-nowrap tabular-nums text-muted">
                {renderValue(item)}
              </span>
            ) : null}
          </li>
        ))}
      </ul>
      {!all && items.length > LIST_PREVIEW ? (
        <button
          type="button"
          onClick={() => setAll(true)}
          className="mt-1 border-0 bg-transparent p-0 text-[0.813rem] text-accent underline-offset-2 hover:underline"
        >
          Show all {items.length.toLocaleString()}
        </button>
      ) : null}
    </>
  );
}

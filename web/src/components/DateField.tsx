import { parseDate } from "@internationalized/date";
import type { ReactNode } from "react";
import {
  Button,
  Calendar,
  CalendarCell,
  CalendarGrid,
  CalendarGridBody,
  CalendarGridHeader,
  CalendarHeaderCell,
  CalendarHeading,
  DateInput,
  DatePicker,
  DateSegment,
  Dialog,
  Group,
  Label,
  Popover,
} from "react-aria-components";

import { popupShadow } from "../lib/uiStyles";
import { Z_POPOVER } from "../lib/zLayers";

function CalendarIcon() {
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
    >
      <rect x="3" y="4" width="18" height="18" rx="2" />
      <path d="M16 2v4" />
      <path d="M8 2v4" />
      <path d="M3 10h18" />
    </svg>
  );
}

/**
 * Date field backed by React Aria's DatePicker + Calendar. Parent value is an
 * ISO YYYY-MM-DD string (or "" for no date); typed and picked dates convert
 * via @internationalized/date.
 */
export default function DateField({
  label,
  value,
  onChange,
  labelClassName = "mb-1 block text-[0.875rem] font-medium text-text",
  groupClassName = "flex items-center rounded border border-border bg-elevated px-2 py-1.5 focus-within:border-accent",
  className,
  pickAriaLabel,
}: {
  label: ReactNode;
  value: string;
  onChange: (value: string) => void;
  labelClassName?: string;
  groupClassName?: string;
  className?: string;
  /** Accessible name for the calendar button when `label` is not a plain string. */
  pickAriaLabel?: string;
}) {
  // parseDate throws on malformed input; parent state is only ever "" or a
  // valid ISO date written by this component, so guard anyway.
  const calendarValue = value && /^\d{4}-\d{2}-\d{2}$/.test(value) ? parseDate(value) : null;
  const pickLabel = pickAriaLabel ?? (typeof label === "string" ? label : "date");

  return (
    <div className={className ?? "min-w-[10rem] flex-[1_1_12rem]"}>
      <DatePicker
        value={calendarValue}
        onChange={(date) => onChange(date ? date.toString() : "")}
        className="flex w-full min-w-0 flex-col"
      >
        <Label className={labelClassName}>{label}</Label>
        <Group className={groupClassName}>
          {/* min-w-0 so segments shrink beside the calendar button instead of
              growing the field past the parent column. */}
          <DateInput className="flex min-w-0 flex-1 overflow-hidden outline-none">
            {(segment) => (
              <DateSegment
                segment={segment}
                className="rounded-sm px-0.5 text-[0.813rem] text-text outline-none data-[placeholder]:text-muted focus:bg-accent focus:text-sent-text"
              />
            )}
          </DateInput>
          <Button
            aria-label={`Pick ${pickLabel}`}
            className="ml-1 flex shrink-0 items-center justify-center rounded border-0 bg-transparent p-0.5 text-muted outline-none hover:text-accent"
          >
            <CalendarIcon />
          </Button>
        </Group>
        <Popover
          data-mc-overlay=""
          className={`rounded-md border border-border bg-popover p-2 outline-none ${Z_POPOVER} ${popupShadow}`}
        >
          <Dialog className="outline-none">
            <Calendar className="outline-none">
              <div className="flex items-center justify-between pb-2">
                <Button
                  slot="previous"
                  className="flex h-6 w-6 items-center justify-center rounded border-0 bg-transparent text-muted outline-none hover:text-accent"
                >
                  ‹
                </Button>
                <CalendarHeading className="text-[0.875rem] font-medium text-text" />
                <Button
                  slot="next"
                  className="flex h-6 w-6 items-center justify-center rounded border-0 bg-transparent text-muted outline-none hover:text-accent"
                >
                  ›
                </Button>
              </div>
              <CalendarGrid className="border-separate border-spacing-1">
                <CalendarGridHeader>
                  {(day) => (
                    <CalendarHeaderCell className="px-1 pb-1 text-center text-[0.75rem] text-muted">
                      {day}
                    </CalendarHeaderCell>
                  )}
                </CalendarGridHeader>
                <CalendarGridBody>
                  {(date) => (
                    <CalendarCell
                      date={date}
                      className={({
                        isHovered,
                        isPressed,
                        isSelected,
                        isFocused,
                        isDisabled,
                        isOutsideMonth,
                      }) =>
                        "flex h-8 w-8 items-center justify-center rounded text-[0.875rem] outline-none " +
                        (isOutsideMonth || isDisabled ? "text-muted opacity-50" : "text-text") +
                        (isHovered || isPressed ? " bg-hover" : "") +
                        (isSelected ? " bg-accent text-sent-text" : "") +
                        (isFocused ? " ring-1 ring-accent ring-inset" : "")
                      }
                    />
                  )}
                </CalendarGridBody>
              </CalendarGrid>
            </Calendar>
          </Dialog>
        </Popover>
      </DatePicker>
    </div>
  );
}

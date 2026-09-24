import { useId } from "react";
import { parseSelectKey } from "../../lib/selectKey";
import DateField from "../DateField";
import Select, { ListBoxItem as SelectListBoxItem } from "../Select";
import {
  compactFieldTriggerClass,
  compactSelectItemClassName,
  dateGroupClass,
  labelClass,
} from "./advancedSearchStyles";
import type { DateBoundFilter, DateBoundOp } from "./buildAdvancedQuery";

/** Compact DateField used under the First/Last Heard operators (label is sr-only). */
function BoundDateInput({
  label,
  pickAriaLabel,
  value,
  onChange,
}: {
  label: string;
  pickAriaLabel: string;
  value: string;
  onChange: (next: string) => void;
}) {
  return (
    <DateField
      label={label}
      pickAriaLabel={pickAriaLabel}
      value={value}
      onChange={onChange}
      labelClassName="sr-only"
      groupClassName={dateGroupClass}
      className="min-w-0 w-full overflow-hidden"
    />
  );
}

/** Operator Select + date field(s) when not Any. */
export default function DateBoundField({
  label,
  value,
  onChange,
  isDisabled = false,
}: {
  label: string;
  value: DateBoundFilter;
  onChange: (next: DateBoundFilter) => void;
  isDisabled?: boolean;
}) {
  const selectId = useId();

  function setOp(op: DateBoundOp): void {
    if (op === "any") {
      onChange({ op: "any", start: "", end: "" });
      return;
    }
    onChange({
      op,
      start: value.start,
      end: op === "between" ? value.end : "",
    });
  }

  let dateControls = null;
  if (!isDisabled && value.op === "between") {
    dateControls = (
      <div className="mt-1.5 grid grid-cols-2 gap-1.5">
        <BoundDateInput
          label="Start"
          pickAriaLabel={`${label} start`}
          value={value.start}
          onChange={(start) => onChange({ ...value, start })}
        />
        <BoundDateInput
          label="End"
          pickAriaLabel={`${label} end`}
          value={value.end}
          onChange={(end) => onChange({ ...value, end })}
        />
      </div>
    );
  } else if (!isDisabled && (value.op === "after" || value.op === "before")) {
    dateControls = (
      <div className="mt-1.5">
        <BoundDateInput
          label="Date"
          pickAriaLabel={label}
          value={value.start}
          onChange={(start) => onChange({ ...value, start })}
        />
      </div>
    );
  }

  return (
    <div className={`min-w-0 ${isDisabled ? "opacity-40" : ""}`}>
      <label htmlFor={selectId} className={labelClass}>
        {label}
      </label>
      <Select
        id={selectId}
        selectedKey={value.op}
        onSelectionChange={(k) => {
          const op = parseSelectKey(k, ["any", "after", "before", "between"] as const);
          if (op) setOp(op);
        }}
        aria-label={`${label} comparison`}
        className="w-full min-w-0"
        size="sm"
        triggerClassName={compactFieldTriggerClass}
        isDisabled={isDisabled}
      >
        <SelectListBoxItem id="any" className={compactSelectItemClassName}>
          Any
        </SelectListBoxItem>
        <SelectListBoxItem id="after" className={compactSelectItemClassName}>
          On or after
        </SelectListBoxItem>
        <SelectListBoxItem id="before" className={compactSelectItemClassName}>
          Before
        </SelectListBoxItem>
        <SelectListBoxItem id="between" className={compactSelectItemClassName}>
          Between
        </SelectListBoxItem>
      </Select>
      {dateControls}
    </div>
  );
}

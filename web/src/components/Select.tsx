import {
  Button,
  Label,
  ListBox,
  ListBoxItem,
  Popover,
  Select as RACSelect,
  type SelectProps,
  SelectValue,
} from "react-aria-components";

import { popupShadow } from "../lib/uiStyles";
import { Z_POPOVER } from "../lib/zLayers";

/** Default control, or the denser one the filter panels use. */
export type SelectSize = "md" | "sm";

const TRIGGER_SIZE: Record<SelectSize, string> = {
  md: "rounded-xl px-3 py-2.5 text-[0.875rem]",
  sm: "rounded-md px-2 py-1 text-[0.813rem]",
};

const ITEM_TEXT: Record<SelectSize, string> = {
  md: "text-[0.875rem]",
  sm: "text-[0.813rem]",
};

/**
 * Shared className render prop for every ListBoxItem inside a Select.
 * Takes the size rather than leaving callers to rewrite the returned string —
 * the compact filter panels used to do that with a `.replace()` on the type
 * size, which would have silently stopped matching if this string changed.
 */
export function selectItemClassName(
  {
    isFocused,
    isSelected,
  }: {
    isFocused: boolean;
    isSelected: boolean;
  },
  size: SelectSize = "md",
): string {
  return (
    `cursor-pointer rounded px-2 py-1 ${ITEM_TEXT[size]} ` +
    (isFocused ? "bg-hover" : "") +
    " " +
    (isSelected ? "bg-accent text-sent-text" : "text-text")
  );
}

/**
 * Shared select wrapping React Aria's Select + Popover + ListBox.
 *
 * Value handling follows React Aria: `selectedKey` / `onSelectionChange`
 * (Key-based) instead of the native `value` / `onChange`.
 */
export default function Select<T extends object>({
  label,
  size = "md",
  triggerClassName,
  valueClassName,
  popoverClassName,
  className,
  children,
  ...props
}: SelectProps<T> & {
  label?: string;
  size?: SelectSize;
  triggerClassName?: string;
  valueClassName?: string;
  popoverClassName?: string;
  className?: string;
}) {
  return (
    <RACSelect {...props} className={className}>
      {label && <Label className="mb-1 block text-[0.875rem] font-medium text-text">{label}</Label>}
      <Button
        className={`box-border flex w-full min-w-0 items-center justify-between gap-2 overflow-hidden border border-border bg-bg font-normal text-text outline-none focus:border-accent ${TRIGGER_SIZE[size]} ${triggerClassName ?? ""}`}
      >
        <SelectValue className={`min-w-0 truncate ${valueClassName ?? ""}`} />
        <svg
          width="10"
          height="10"
          viewBox="0 0 10 10"
          aria-hidden="true"
          className="shrink-0 text-muted"
        >
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
      <Popover
        data-mc-overlay=""
        className={`box-border w-[var(--trigger-width)] max-w-[var(--trigger-width)] rounded-md border border-border bg-popover p-1 outline-none ${Z_POPOVER} ${popupShadow} ${popoverClassName ?? ""}`}
      >
        <ListBox className="max-h-72 overflow-auto outline-none">{children}</ListBox>
      </Popover>
    </RACSelect>
  );
}

export { ListBoxItem };

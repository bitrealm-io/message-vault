import { useEffect, useRef, useState } from "react";
import {
  Button as AriaButton,
  type Key,
  Label,
  ListBox,
  ListBoxItem,
  Popover,
  Select as RACSelect,
} from "react-aria-components";
import { popupShadow } from "../../lib/uiStyles";
import { Z_POPOVER } from "../../lib/zLayers";
import { compactSelectItemClassName, labelClass } from "./advancedSearchStyles";

/** The transports the search language knows, named the way people name them. */
const SERVICE_ITEMS = [
  { id: "imessage", name: "iMessage" },
  { id: "sms", name: "SMS" },
  { id: "mms", name: "MMS" },
  { id: "rcs", name: "RCS" },
  { id: "whatsapp", name: "WhatsApp" },
] as const;

/**
 * Multi-select without search — click the whole field to open.
 *
 * Uses a non-modal popover (no full-screen underlay) plus a controlled open
 * state and document mousedown listener. That lets one click close the list
 * and activate the clicked form control, without the modal underlay racing
 * Advanced Search's own outside-click dismiss handler.
 */
export default function ServiceMultiSelect({
  value,
  onChange,
  isDisabled = false,
}: {
  value: Key[];
  onChange: (keys: Key[]) => void;
  isDisabled?: boolean;
}) {
  const [isOpen, setIsOpen] = useState(false);
  const selectRef = useRef<HTMLDivElement>(null);
  const popoverRef = useRef<HTMLElement>(null);

  const selectedLabels = SERVICE_ITEMS.filter((item) => value.includes(item.id))
    .map((item) => item.name)
    .join(", ");

  useEffect(() => {
    if (isDisabled) setIsOpen(false);
  }, [isDisabled]);

  useEffect(() => {
    if (!isOpen) return;
    const onPointerDown = (e: MouseEvent) => {
      const target = e.target;
      if (!(target instanceof Node)) return;
      if (selectRef.current?.contains(target)) return;
      if (popoverRef.current?.contains(target)) return;
      // Close only the Service list. Do not stop the click, so it can still
      // activate Search, another field, or close Advanced Search.
      setIsOpen(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, [isOpen]);

  return (
    <RACSelect<object, "multiple">
      ref={selectRef}
      selectionMode="multiple"
      shouldCloseOnSelect={false}
      isOpen={isDisabled ? false : isOpen}
      onOpenChange={(open) => {
        if (isDisabled) return;
        setIsOpen(open);
      }}
      isDisabled={isDisabled}
      value={value}
      onChange={onChange}
      placeholder="Any"
      className={`w-full min-w-0 ${isDisabled ? "opacity-40" : ""}`}
    >
      <Label className={labelClass}>Service</Label>
      <AriaButton
        className={`box-border flex w-full min-w-0 items-center justify-between gap-2 overflow-hidden rounded-md border border-border bg-bg px-2 py-1 text-[0.813rem] text-text outline-none focus:border-accent ${
          isDisabled ? "cursor-not-allowed" : ""
        }`}
      >
        <span className="min-w-0 truncate text-muted">{value.length > 0 ? "Select…" : "Any"}</span>
        <svg
          width="10"
          height="10"
          viewBox="0 0 10 10"
          aria-hidden
          className="ml-1 shrink-0 text-muted"
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
      </AriaButton>
      <Popover
        ref={popoverRef}
        data-mc-overlay=""
        isNonModal
        className={`box-border w-[var(--trigger-width)] max-w-[var(--trigger-width)] rounded-md border border-border bg-popover p-1 outline-none ${Z_POPOVER} ${popupShadow}`}
      >
        <ListBox className="outline-none">
          {SERVICE_ITEMS.map((item) => (
            <ListBoxItem
              key={item.id}
              id={item.id}
              textValue={item.name}
              className={compactSelectItemClassName}
            >
              {({ isSelected }) => (
                <div className="flex items-center gap-2">
                  <span
                    aria-hidden
                    className={`inline-flex h-3.5 w-3.5 items-center justify-center rounded border text-[0.625rem] ${
                      isSelected
                        ? "border-accent bg-accent text-sent-text"
                        : "border-border bg-bg text-transparent"
                    }`}
                  >
                    ✓
                  </span>
                  {item.name}
                </div>
              )}
            </ListBoxItem>
          ))}
        </ListBox>
      </Popover>
      {selectedLabels ? (
        <div className="mt-1 text-[0.75rem] leading-snug text-muted">{selectedLabels}</div>
      ) : null}
    </RACSelect>
  );
}

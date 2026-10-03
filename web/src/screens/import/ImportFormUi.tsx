import type { ReactNode } from "react";
import { Button, Disclosure, DisclosurePanel } from "react-aria-components";
import FormField from "../../components/FormField";
import { textInputClassName } from "../../components/TextField";
import type { AttachmentMediaMode } from "../../lib/types";

export const ATTACHMENT_OPTIONS: { id: AttachmentMediaMode; label: string }[] = [
  { id: "copy", label: "Copy" },
  { id: "convert", label: "Convert" },
  { id: "compress", label: "Compress & Convert" },
  { id: "skip", label: "Skip" },
];

export const RESOLUTION_OPTIONS = ["720p", "1080p", "4k"];

export const fieldStyle = textInputClassName;
export const hintStyle = "mt-1 text-[0.75rem] text-muted";

export const sectionGap = "mb-[1.1rem]";

/** Stacked label + control; thin wrap around FormField. */
export function StackedField({
  label,
  children,
  trailing,
  required,
  optional,
}: {
  label: string;
  children: ReactNode;
  trailing?: ReactNode;
  required?: boolean;
  optional?: boolean;
}) {
  return (
    <FormField
      label={label}
      layout="stacked"
      trailing={trailing}
      required={required}
      optional={optional}
    >
      {children}
    </FormField>
  );
}

export function CollapsibleSection({
  title,
  open,
  onToggle,
  children,
}: {
  title: string;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <Disclosure
      isExpanded={open}
      onExpandedChange={onToggle}
      className={`block ${open ? "mb-3" : "mb-5"}`}
    >
      {({ isExpanded }) => (
        <>
          <Button
            slot="trigger"
            className="flex w-full cursor-pointer items-center gap-2 rounded-none border-0 border-b border-border bg-transparent p-0 pb-2 pt-1 text-left text-[0.9375rem] font-semibold text-text outline-none hover:text-accent focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-1"
          >
            <span
              aria-hidden
              className={`inline-block text-[0.75rem] leading-none text-muted transition-transform duration-150 ${isExpanded ? "rotate-90" : ""}`}
            >
              ▶
            </span>
            <span>{title}</span>
          </Button>
          <DisclosurePanel className="mt-3 ml-3 outline-none">{children}</DisclosurePanel>
        </>
      )}
    </Disclosure>
  );
}

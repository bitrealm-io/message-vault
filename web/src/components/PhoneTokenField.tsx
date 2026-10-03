import { forwardRef, useImperativeHandle, useRef, useState } from "react";
import { Button, Tag, TagGroup, TagList } from "react-aria-components";
import { commitPhoneTokens, removePhoneToken } from "../lib/phoneTokens";
import { textInputClassName } from "./TextField";

export type PhoneTokenFieldHandle = {
  /** Commit any in-progress draft and return the resulting phone list. */
  flush: () => string[];
};

export type PhoneTokenFieldProps = {
  value: string[];
  onChange: (phones: string[]) => void;
  /** Fired when the uncommitted draft text changes. */
  onDraftChange?: (draft: string) => void;
  "aria-label"?: string;
  placeholder?: string;
};

/**
 * Multi-value phone field: each number is a removable token.
 * Enter, comma, or blur commits the draft; Backspace on empty draft removes the last token.
 * Call `flush()` before Import so a typed-but-uncommitted number is not dropped.
 */
const PhoneTokenField = forwardRef<PhoneTokenFieldHandle, PhoneTokenFieldProps>(
  function PhoneTokenField(
    {
      value,
      onChange,
      onDraftChange,
      "aria-label": ariaLabel = "Backup Device Phone Numbers",
      placeholder = "+1 555-555-0119",
    },
    ref,
  ) {
    const [draft, setDraft] = useState("");
    const draftRef = useRef(draft);
    const valueRef = useRef(value);
    draftRef.current = draft;
    valueRef.current = value;

    function setDraftValue(next: string): void {
      draftRef.current = next;
      setDraft(next);
      onDraftChange?.(next);
    }

    function commitRaw(raw: string, phones: string[]): string[] {
      const trimmed = raw.trim();
      if (!trimmed) {
        setDraftValue("");
        return phones;
      }
      const next = commitPhoneTokens(phones, trimmed);
      setDraftValue("");
      onChange(next);
      return next;
    }

    useImperativeHandle(ref, () => ({
      flush: () => commitRaw(draftRef.current, valueRef.current),
    }));

    return (
      <div
        className={`${textInputClassName} flex min-h-[2.75rem] flex-wrap items-center gap-1.5 py-1.5 focus-within:border-accent`}
      >
        <TagGroup
          aria-label={ariaLabel}
          onRemove={(keys) => {
            const key = [...keys][0];
            if (typeof key === "string") onChange(removePhoneToken(value, key));
          }}
        >
          <TagList className="contents">
            {value.map((phone) => (
              <Tag
                key={phone}
                id={phone}
                textValue={phone}
                className="inline-flex items-center gap-1 rounded-lg border border-border bg-panel px-2 py-0.5 text-[0.8125rem] text-text outline-none focus-visible:ring-2 focus-visible:ring-accent data-[selected]:border-accent"
              >
                {phone}
                <Button
                  slot="remove"
                  className="cursor-pointer border-0 bg-transparent p-0 text-muted hover:text-text"
                  aria-label={`Remove ${phone}`}
                >
                  ×
                </Button>
              </Tag>
            ))}
          </TagList>
        </TagGroup>
        <input
          type="text"
          value={draft}
          aria-label="Add phone number"
          placeholder={value.length === 0 ? placeholder : "Add another"}
          className="min-w-[8rem] flex-1 border-0 bg-transparent p-0 text-[0.875rem] text-text outline-none"
          onChange={(e) => {
            const next = e.target.value;
            if (next.includes(",")) {
              commitRaw(next, value);
              return;
            }
            setDraftValue(next);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              commitRaw(draftRef.current, valueRef.current);
              return;
            }
            if (e.key === "Backspace" && draft.length === 0 && value.length > 0) {
              e.preventDefault();
              onChange(value.slice(0, -1));
            }
          }}
          onBlur={() => {
            commitRaw(draftRef.current, valueRef.current);
          }}
        />
      </div>
    );
  },
);

export default PhoneTokenField;

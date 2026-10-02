import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import Button from "./Button";
import { textInputClassName } from "./TextField";

interface PathPickerProps {
  value: string;
  onChange: (path: string) => void;
  /** Called when the text field loses focus. */
  onBlur?: () => void;
  directory?: boolean;
  placeholder?: string;
  /** Forwarded to the text field so a wrapping label can focus the input. */
  id?: string;
  filters?: { name: string; extensions: string[] }[];
  /** Locks both the text field and Browse, as while a job writes to the path. */
  isDisabled?: boolean;
}

export default function PathPicker({
  value,
  onChange,
  onBlur,
  directory,
  placeholder,
  id,
  filters,
  isDisabled,
}: PathPickerProps) {
  const [browseError, setBrowseError] = useState("");

  const browse = async () => {
    setBrowseError("");
    let result: string | string[] | null;
    try {
      result = directory
        ? await open({ directory: true, multiple: false })
        : await open({ multiple: false, filters });
    } catch (err) {
      // A Tauri command rejects with the plugin's own string, not an Error.
      const reason = err instanceof Error ? err.message : String(err);
      setBrowseError(`The file dialog could not be opened: ${reason}`);
      return;
    }
    if (result && typeof result === "string") {
      onChange(result);
    }
  };

  return (
    <div className="flex flex-1 flex-col gap-1">
      <div className="flex gap-2">
        <input
          id={id}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onBlur={onBlur}
          placeholder={placeholder}
          spellCheck={false}
          autoComplete="off"
          disabled={isDisabled}
          className={`flex-1 ${textInputClassName}`}
        />
        <Button onClick={browse} size="xs" disabled={isDisabled}>
          Browse
        </Button>
      </div>
      {browseError ? (
        <p role="alert" className="m-0 text-[0.75rem] text-danger">
          {browseError}
        </p>
      ) : null}
    </div>
  );
}

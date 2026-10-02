import { open } from "@tauri-apps/plugin-dialog";
import Button from "./Button";
import { textInputClassName } from "./TextField";

interface PathPickerProps {
  value: string;
  onChange: (path: string) => void;
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
  directory,
  placeholder,
  id,
  filters,
  isDisabled,
}: PathPickerProps) {
  const browse = async () => {
    const result = directory
      ? await open({ directory: true, multiple: false })
      : await open({ multiple: false, filters });
    if (result && typeof result === "string") {
      onChange(result);
    }
  };

  return (
    <div className="flex flex-1 gap-2">
      <input
        id={id}
        value={value}
        onChange={(e) => onChange(e.target.value)}
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
  );
}

import { useId, useState } from "react";
import { normalizeHex, type ThemeSeeds } from "../../lib/theme";

export function ColorRow({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (hex: string) => void;
}) {
  const colorId = useId();
  const [text, setText] = useState(value);
  const [prevValue, setPrevValue] = useState(value);
  if (value !== prevValue) {
    setPrevValue(value);
    setText(value);
  }

  return (
    <div className="flex items-center gap-3">
      <label htmlFor={colorId} className="w-[7rem] shrink-0 text-[0.813rem] text-muted">
        {label}
      </label>
      <input
        id={colorId}
        type="color"
        value={normalizeHex(value) ?? "#000000"}
        onChange={(e) => onChange(e.target.value)}
        aria-label={label}
        className="h-9 w-12 cursor-pointer rounded-md border border-border bg-panel p-0.5"
      />
      <input
        type="text"
        value={text}
        spellCheck={false}
        aria-label={`${label} hex`}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => {
          const hex = normalizeHex(text);
          if (hex) onChange(hex);
          else setText(value);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            (e.target as HTMLInputElement).blur();
          }
        }}
        className="min-w-0 flex-1 rounded-md border border-border bg-bg px-2.5 py-1.5 font-mono text-[0.813rem] text-text outline-none focus:border-accent"
      />
    </div>
  );
}

export function formatCompare(seeds: ThemeSeeds): string {
  return [seeds.lightHeader, seeds.lightAccent, seeds.darkHeader, seeds.darkAccent]
    .map((h) => h.toLowerCase())
    .join(",");
}

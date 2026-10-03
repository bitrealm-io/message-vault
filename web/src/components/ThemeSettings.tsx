import { useState } from "react";
import { Checkbox, Radio, RadioGroup, ToggleButton } from "react-aria-components";
import { parseSelectKey } from "../lib/selectKey";
import { useTheme } from "../lib/ThemeProvider";
import type { ThemeSeeds } from "../lib/theme";
import { ColorRow, formatCompare } from "./theme/ThemeColorRow";

const SEED_FIELDS: {
  key: keyof ThemeSeeds;
  label: string;
}[] = [
  { key: "lightHeader", label: "Light header" },
  { key: "lightAccent", label: "Light accent" },
  { key: "darkHeader", label: "Dark header" },
  { key: "darkAccent", label: "Dark accent" },
];

const MODE_OPTIONS = [
  { value: "light" as const, label: "Light" },
  { value: "dark" as const, label: "Dark" },
];

const sectionTitleClass = "m-0 text-[0.75rem] font-semibold uppercase tracking-[0.05em] text-muted";

const mutedTextClass = "mt-1 text-[0.813rem] text-muted";

/** Check glyph shared by the mode radio dots and the system-mode checkbox. */
function CheckIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.25"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      className={className}
    >
      <path d="M3.5 8.5 6.5 11.5 12.5 4.5" />
    </svg>
  );
}

export default function ThemeSettings() {
  const {
    mode,
    setMode,
    seeds,
    patchSeed,
    shareString,
    setShareString,
    applyPreset,
    resolvedMode,
    presets,
  } = useTheme();

  const [shareDraft, setShareDraft] = useState(shareString);
  const [prevShareString, setPrevShareString] = useState(shareString);
  const [shareError, setShareError] = useState(false);
  const [copied, setCopied] = useState(false);
  // Browsers expose navigator.clipboard only on HTTPS and localhost, and can
  // refuse the write even there. The codes stay in the field to copy by hand.
  const [copyFailed, setCopyFailed] = useState(false);

  if (shareString !== prevShareString) {
    setPrevShareString(shareString);
    setShareDraft(shareString);
    setShareError(false);
  }

  const matchSystem = mode === "system";

  return (
    <section className="max-w-[36rem]">
      <h2 className={sectionTitleClass}>Theme</h2>
      <p className={mutedTextClass}>
        Choose colors for light and dark mode. Changes save automatically.
      </p>

      <RadioGroup
        // Keep one card selected. A group value of "system" matches no radio
        // and would make the picker unreachable from the keyboard.
        value={resolvedMode}
        onChange={(value) => {
          const mode = parseSelectKey(value, ["light", "dark"] as const);
          if (mode) setMode(mode);
        }}
        aria-label="Color mode"
        className="mt-4 grid grid-cols-[repeat(auto-fit,minmax(10rem,1fr))] gap-2"
      >
        {MODE_OPTIONS.map((opt) => (
          <Radio
            key={opt.value}
            value={opt.value}
            className={({ isSelected }) =>
              `cursor-pointer overflow-hidden rounded-lg border text-left outline-none
               data-focus-visible:ring-2 data-focus-visible:ring-accent
               ${isSelected ? "border-accent" : "border-border"}`
            }
          >
            {({ isSelected }) => (
              <>
                <div
                  className="flex h-20 items-end gap-1 px-3 pb-2"
                  style={{
                    background: opt.value === "light" ? seeds.lightHeader : seeds.darkHeader,
                  }}
                >
                  <span
                    className="h-6 flex-1 rounded-[2px]"
                    style={{
                      background: opt.value === "light" ? seeds.lightAccent : seeds.darkAccent,
                    }}
                  />
                  <span className="h-6 w-10 rounded-[2px] bg-text opacity-80" />
                </div>
                <div className="flex items-center gap-2 bg-panel px-3 py-2">
                  <span
                    aria-hidden
                    className={`flex h-5 w-5 shrink-0 items-center justify-center rounded-full border-2 ${
                      isSelected
                        ? "border-accent bg-accent text-sent-text"
                        : "border-border bg-panel"
                    }`}
                  >
                    {isSelected ? <CheckIcon className="h-3 w-3" /> : null}
                  </span>
                  <span className="text-[0.875rem] font-medium text-text">{opt.label}</span>
                </div>
              </>
            )}
          </Radio>
        ))}
      </RadioGroup>

      <Checkbox
        isSelected={matchSystem}
        onChange={(checked) => {
          if (checked) setMode("system");
          else setMode(resolvedMode);
        }}
        className="mt-4 flex cursor-pointer items-center gap-2.5 text-[0.875rem] text-text outline-none data-focus-visible:ring-2 data-focus-visible:ring-accent"
      >
        {({ isSelected }) => (
          <>
            <span
              className={`flex h-4 w-4 shrink-0 items-center justify-center rounded border ${
                isSelected
                  ? "border-accent bg-accent text-[color:var(--checkbox-check)]"
                  : "border-border bg-elevated"
              }`}
            >
              {isSelected ? <CheckIcon className="h-3.5 w-3.5" /> : null}
            </span>
            Match your device theme
          </>
        )}
      </Checkbox>

      <div className="mt-6 flex flex-col gap-3">
        <div className={sectionTitleClass}>Colors</div>
        {SEED_FIELDS.map((field) => (
          <ColorRow
            key={field.key}
            label={field.label}
            value={seeds[field.key]}
            onChange={(hex) => patchSeed(field.key, hex)}
          />
        ))}
      </div>

      <div className="mt-6">
        <div className={sectionTitleClass}>Share theme</div>
        <p className={mutedTextClass}>Copy or paste the four color codes below.</p>
        <div className="mt-2 flex gap-2">
          <input
            type="text"
            value={shareDraft}
            spellCheck={false}
            onChange={(e) => {
              setShareDraft(e.target.value);
              setShareError(false);
            }}
            onBlur={() => {
              if (shareDraft.trim() === shareString) return;
              const ok = setShareString(shareDraft);
              setShareError(!ok);
              if (!ok) setShareDraft(shareString);
            }}
            aria-invalid={shareError}
            className={`min-w-0 flex-1 rounded-md border bg-bg px-2.5 py-1.5 font-mono text-[0.75rem] text-text outline-none focus:border-accent ${
              shareError ? "border-danger" : "border-border"
            }`}
          />
          <button
            type="button"
            onClick={async () => {
              try {
                await navigator.clipboard.writeText(shareString);
                setCopied(true);
                setCopyFailed(false);
                window.setTimeout(() => setCopied(false), 1500);
              } catch {
                setCopyFailed(true);
              }
            }}
            className="shrink-0 cursor-pointer rounded-md border border-border bg-panel px-3 py-1.5 text-[0.813rem] text-text"
          >
            {copied ? "Copied" : "Copy"}
          </button>
        </div>
        {shareError ? (
          <p className="mt-1 text-[0.75rem] text-danger" role="alert">
            Enter four valid color codes.
          </p>
        ) : null}
        {copyFailed ? (
          <p className="mt-1 text-[0.75rem] text-text" role="alert">
            This browser can&apos;t copy from this page. Select the color codes above and copy them.
          </p>
        ) : null}
      </div>

      <div className="mt-6">
        <div className={sectionTitleClass}>Tried and true</div>
        <div className="mt-3 flex flex-wrap gap-3">
          {presets.map((preset) => {
            const active = formatCompare(seeds) === formatCompare(preset.seeds);
            return (
              <ToggleButton
                key={preset.id}
                isSelected={active}
                onChange={() => applyPreset(preset)}
                aria-label={preset.label}
                ref={(el) => {
                  // The menu library strips `title`. Set it on the element so hover text still works.
                  if (el && el.title !== preset.label) el.title = preset.label;
                }}
                className={({ isSelected }) =>
                  `relative h-10 w-10 cursor-pointer rounded-full border-2 outline-none
                   focus-visible:ring-2 focus-visible:ring-accent
                   ${isSelected ? "border-accent" : "border-transparent"}`
                }
                style={{
                  background: `conic-gradient(
                    ${preset.seeds.lightHeader} 0deg 90deg,
                    ${preset.seeds.lightAccent} 90deg 180deg,
                    ${preset.seeds.darkHeader} 180deg 270deg,
                    ${preset.seeds.darkAccent} 270deg 360deg
                  )`,
                }}
              />
            );
          })}
        </div>
      </div>
    </section>
  );
}

import { useCallback, useState } from "react";
import { ListBoxItem } from "react-aria-components";
import FormRow from "../../components/FormRow";
import PathPicker from "../../components/PathPicker";
import Select, { selectItemClassName } from "../../components/Select";
import TauriJobFormShell from "../../components/TauriJobFormShell";
import { useTauriJob } from "../../hooks/useTauriJob";
import { parseSelectKey } from "../../lib/selectKey";
import { EXPORT_FORMATS, type ExportFormat, invokeFormat } from "../../lib/tauri";
import { isTauri } from "../../lib/tauri-check";
import { sameFolder } from "./convertUtils";

const FORMAT_IDS = EXPORT_FORMATS.map((f) => f.id);

/** Label for the chosen format, for the success panel. */
function formatLabel(id: ExportFormat): string {
  return EXPORT_FORMATS.find((f) => f.id === id)?.label ?? id;
}

/**
 * Settings → Convert: rewrite a folder of already-exported files into another
 * format. `message-reexport` detects the input format from the folder, so the
 * screen picks the output format only, and it refuses to write into its own
 * input, so the two folders must differ.
 *
 * Convert reads files and writes files. It never opens a backup or the server,
 * which is why it lives under Settings as a tool rather than in the sidebar
 * beside Import and Export.
 */
export function ConvertSection() {
  const [inputDir, setInputDir] = useState("");
  const [outputDir, setOutputDir] = useState("");
  const [format, setFormat] = useState<ExportFormat>("jsonl");
  const [error, setError] = useState("");
  const [log, setLog] = useState<string[]>([]);
  const { running, finished, run, cancel } = useTauriJob();

  const appendLog = useCallback((line: string) => {
    setLog((prev) => [...prev, line]);
  }, []);

  if (!isTauri()) {
    return (
      <p className="m-0 text-[0.875rem] text-muted">
        Convert rewrites a folder of exported files into another format. It is available in the
        desktop app.
      </p>
    );
  }

  const foldersClash = sameFolder(inputDir, outputDir);

  const startConvert = () => {
    if (running || foldersClash) return;
    setError("");
    setLog([]);
    void (async () => {
      try {
        await run(
          () =>
            invokeFormat({
              input_dir: inputDir.trim(),
              output_dir: outputDir.trim(),
              output_format: format,
            }),
          { onLog: appendLog },
        );
      } catch (err: unknown) {
        const message = err instanceof Error ? err.message : String(err);
        appendLog(`Error: ${message}`);
        setError(message);
      }
    })();
  };

  return (
    <TauriJobFormShell
      className="max-w-[700px]"
      startLabel="Convert"
      runningLabel="Converting…"
      running={running}
      log={log}
      startDisabled={!inputDir.trim() || !outputDir.trim() || foldersClash}
      onStart={startConvert}
      onCancel={cancel}
      error={error}
      intro={
        <p className="mb-6 text-[0.875rem] text-muted">
          Convert rewrites a folder of exported files into another format. The input format is read
          from the folder. Convert touches neither a backup nor your Message Crate.
        </p>
      }
      success={
        finished && !error ? (
          <div className="mt-4 rounded-md bg-ok-soft-bg p-4 text-[0.875rem]">
            Conversion complete. {formatLabel(format)} written to {outputDir.trim()}.
          </div>
        ) : null
      }
    >
      <FormRow label="Input folder">
        <PathPicker
          value={inputDir}
          onChange={setInputDir}
          directory
          placeholder="Folder holding an export…"
        />
      </FormRow>
      <FormRow label="Output folder">
        <PathPicker
          value={outputDir}
          onChange={setOutputDir}
          directory
          placeholder="A different folder to write into…"
        />
      </FormRow>
      {foldersClash ? (
        <p role="alert" className="mb-3 ml-[calc(140px+0.75rem)] text-[0.813rem] text-danger">
          Choose a different output folder. Convert can't write into the folder it reads from, so
          the two folders must differ.
        </p>
      ) : null}
      <FormRow label="Output format">
        <Select
          selectedKey={format}
          onSelectionChange={(key) => {
            const next = parseSelectKey(key, FORMAT_IDS);
            if (next) setFormat(next);
          }}
          aria-label="Output format"
          isDisabled={running}
        >
          {EXPORT_FORMATS.map((option) => (
            <ListBoxItem key={option.id} id={option.id} className={selectItemClassName}>
              {option.label}
            </ListBoxItem>
          ))}
        </Select>
      </FormRow>
    </TauriJobFormShell>
  );
}

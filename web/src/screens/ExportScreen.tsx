import { useCallback, useRef, useState } from "react";
import { ListBoxItem } from "react-aria-components";
import { useSearchParams } from "react-router-dom";
import FormRow from "../components/FormRow";
import PathPicker from "../components/PathPicker";
import Select, { selectItemClassName } from "../components/Select";
import TauriJobFormShell from "../components/TauriJobFormShell";
import TextField from "../components/TextField";
import { useTauriJob } from "../hooks/useTauriJob";
import { getBaseUrl } from "../lib/api";
import { useAuth } from "../lib/auth";
import { createRunCancel, type RunCancel } from "../lib/runCancel";
import { parseSelectKey } from "../lib/selectKey";
import { resolveExportStagingDir } from "../lib/system-settings";
import {
  EXPORT_FORMATS,
  type ExportFormat,
  type ExportQueryList,
  invokeDeleteStaging,
  invokeFormat,
  invokePull,
} from "../lib/tauri";

const FORMAT_IDS = EXPORT_FORMATS.map((f) => f.id);

/**
 * What an export covers. `everything` sends a blank query, which message-crate-pull
 * reads as the whole account; `search` sends the text of the query box.
 */
type ExportScope = "everything" | "search";

const SCOPES: { id: ExportScope; label: string }[] = [
  { id: "everything", label: "Everything" },
  { id: "search", label: "Search" },
];
const SCOPE_IDS = SCOPES.map((s) => s.id);

/**
 * The list a search is for, and what the export then holds, in the words the
 * screen shows under the query box. The Conversations list takes its own
 * search words (`messages:>100`) and the export is whole conversations; the
 * Messages list takes its own (`from:me`, `in:#19`) and the export is the
 * matching messages alone.
 */
const QUERY_LISTS: { id: ExportQueryList; label: string; holds: string }[] = [
  {
    id: "conversations",
    label: "Conversations",
    holds:
      "The export holds every message of each conversation this search finds. messages:>100 finds the long ones.",
  },
  {
    id: "messages",
    label: "Messages",
    holds:
      "The export holds only the messages this search finds. in:#19,#22 names two conversations by their ids.",
  },
];
const QUERY_LIST_IDS = QUERY_LISTS.map((l) => l.id);

/** Label for the chosen format, for the success panel. */
function formatLabel(id: ExportFormat): string {
  return EXPORT_FORMATS.find((f) => f.id === id)?.label ?? id;
}

/**
 * Desktop export: `message-crate-pull` downloads the account's conversations into a folder, and for any
 * format other than JSONL `message-reexport` rewrites that folder into the
 * chosen format.
 *
 * The two steps need two folders. `message-crate-pull` only writes JSONL, and
 * `message-reexport` refuses to convert a folder into itself, so a non-JSONL
 * export pulls into a staging folder first and converts out of it into the
 * folder the person picked. The staging folder is deleted either way, so a
 * failed conversion does not leave a copy of the conversations behind.
 *
 * The scope is Everything or Search. The screen opens in Search when its URL
 * carries `?q=`: LeftPanel puts the query the conversation list was browsing
 * with there when the person clicks Export, with `list=conversations` beside
 * it, so "export what I am looking at" is one click: the query box shows
 * the search, and the export holds every message of the conversations that
 * list showed. A search typed here without that hand-off is for the Messages
 * list, where `in:#19,#22` names chosen conversations. The line under the
 * query box says which of the two the file will hold.
 *
 * Shown only when Tauri is available (see LeftPanel).
 */
export default function ExportScreen() {
  const { token } = useAuth();
  const [searchParams] = useSearchParams();
  const browsedQuery = (searchParams.get("q") ?? "").trim();
  const [scope, setScope] = useState<ExportScope>(browsedQuery ? "search" : "everything");
  const [query, setQuery] = useState(browsedQuery);
  const [list, setList] = useState<ExportQueryList>(
    searchParams.get("list") === "conversations" ? "conversations" : "messages",
  );
  const [savePath, setSavePath] = useState("");
  const [format, setFormat] = useState<ExportFormat>("jsonl");
  const [error, setError] = useState("");
  const [log, setLog] = useState<string[]>([]);
  // `running` only turns true once a job starts, which leaves two windows
  // where the Export button would be live mid-export: while the staging path
  // resolves (a `home_dir` round trip on the first export), and between the
  // pull and the conversion. A second job started in either window would
  // break `jobs.rs`'s "one job runs at a time" assumption, and two exports
  // begun in the same second would share a staging folder, so the first
  // cleanup would delete the second's files. This covers the whole run.
  const [busy, setBusy] = useState(false);
  const { running, finished, run } = useTauriJob();
  // The Cancel of the export under way. A Cancel pressed after the pull and
  // before the conversion starts must stop the conversion, and the shared
  // cancel flag alone would not: `format` clears it when it starts.
  const runCancel = useRef<RunCancel>(createRunCancel());

  const appendLog = useCallback((line: string) => {
    setLog((prev) => [...prev, line]);
  }, []);

  const startExport = () => {
    if (busy) return;
    if (!token) {
      setError("Not authenticated");
      return;
    }
    setBusy(true);
    setError("");
    setLog([]);
    const exportCancel = createRunCancel();
    runCancel.current = exportCancel;

    const pullInto = (outDir: string) =>
      run(
        exportCancel.guard(() =>
          invokePull({
            base_url: getBaseUrl(),
            username: "",
            key: token,
            out_dir: outDir,
            query: scope === "search" ? query.trim() : "",
            list,
            skip_attachments: false,
          }),
        ),
        { onLog: appendLog },
      );

    void (async () => {
      try {
        if (format === "jsonl") {
          await pullInto(savePath);
          return;
        }
        const stagingDir = await resolveExportStagingDir();
        try {
          await pullInto(stagingDir);
          await run(
            exportCancel.guard(() =>
              invokeFormat({
                input_dir: stagingDir,
                output_dir: savePath,
                output_format: format,
              }),
            ),
            { onLog: appendLog },
          );
        } finally {
          // Best effort: a staging folder left behind is worth a log line, not
          // a failed export the person cannot tell apart from a real one.
          try {
            await invokeDeleteStaging({ staging_dir: stagingDir });
          } catch (cleanupError: unknown) {
            appendLog(
              `Could not remove the staging folder ${stagingDir}: ${
                cleanupError instanceof Error ? cleanupError.message : String(cleanupError)
              }`,
            );
          }
        }
      } catch (err: unknown) {
        const message = err instanceof Error ? err.message : String(err);
        appendLog(`Error: ${message}`);
        setError(message);
      } finally {
        setBusy(false);
      }
    })();
  };

  return (
    <TauriJobFormShell
      title="Export"
      requireTauri
      startLabel="Export"
      runningLabel="Exporting…"
      running={running || busy}
      log={log}
      startDisabled={!savePath || busy || (scope === "search" && query.trim() === "")}
      onStart={startExport}
      onCancel={() => void runCancel.current.cancel()}
      error={error}
      intro={
        <p className="mb-6 text-[0.875rem] text-muted">
          Export every conversation, or only the ones a search finds, into a folder in the format
          you choose. Attachments come with the messages.
        </p>
      }
      success={
        finished && !error ? (
          <div className="mt-4 rounded-md bg-ok-soft-bg p-4 text-[0.875rem]">
            Export complete. {formatLabel(format)} saved to {savePath}.
          </div>
        ) : null
      }
    >
      <FormRow label="Scope">
        <Select
          selectedKey={scope}
          onSelectionChange={(key) => {
            const next = parseSelectKey(key, SCOPE_IDS);
            if (next) setScope(next);
          }}
          aria-label="Scope"
          isDisabled={running || busy}
        >
          {SCOPES.map((option) => (
            <ListBoxItem key={option.id} id={option.id} className={selectItemClassName}>
              {option.label}
            </ListBoxItem>
          ))}
        </Select>
      </FormRow>
      {scope === "search" ? (
        <>
          <FormRow label="Search in">
            <Select
              selectedKey={list}
              onSelectionChange={(key) => {
                const next = parseSelectKey(key, QUERY_LIST_IDS);
                if (next) setList(next);
              }}
              aria-label="Search in"
              isDisabled={running || busy}
            >
              {QUERY_LISTS.map((option) => (
                <ListBoxItem key={option.id} id={option.id} className={selectItemClassName}>
                  {option.label}
                </ListBoxItem>
              ))}
            </Select>
          </FormRow>
          <FormRow label="Search">
            <TextField
              aria-label="Search"
              value={query}
              onChange={setQuery}
              isDisabled={running || busy}
              placeholder={list === "conversations" ? "tag:Work last year" : "from:me last year"}
              hint={QUERY_LISTS.find((l) => l.id === list)?.holds}
            />
          </FormRow>
        </>
      ) : null}
      <FormRow label="Save to">
        <PathPicker
          value={savePath}
          onChange={setSavePath}
          directory
          placeholder="Choose folder…"
        />
      </FormRow>
      <FormRow label="Format">
        <Select
          selectedKey={format}
          onSelectionChange={(key) => {
            const next = parseSelectKey(key, FORMAT_IDS);
            if (next) setFormat(next);
          }}
          aria-label="Format"
          isDisabled={running || busy}
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

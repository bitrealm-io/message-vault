import { useCallback, useEffect, useId, useRef, useState } from "react";
import Checkbox from "../../components/Checkbox";
import { CheckIcon, XIcon } from "../../components/icons";
import PathPicker from "../../components/PathPicker";
import { getBaseUrl } from "../../lib/api";
import { APP_BUILD } from "../../lib/build";
import { FFMPEG_TOOLS_STORAGE_KEY } from "../../lib/ffmpeg-tools";
import {
  getOpenToNetwork,
  isOwnAddress,
  type LocalServerStatus,
  openDataFolder,
  setLocalServerOpenToNetwork,
  setOpenToNetwork,
} from "../../lib/localServer";
import { readPref, removePref, writePref } from "../../lib/storage";
import { getRememberImporterPaths, setRememberImporterPaths } from "../../lib/system-settings";
import {
  type FfmpegToolsProbe,
  invokeSetStagingRoot,
  invokeStagingRoot,
  probeFfmpegTools,
  setFfmpegToolsDir,
} from "../../lib/tauri";
import { isTauri } from "../../lib/tauri-check";
import { readerLicenseUrl, readerSourceUrl } from "../../lib/thirdPartySoftware";

const sectionHeading = "m-0 mb-2 text-[12px] font-semibold uppercase tracking-[0.05em] text-muted";

const EXAMPLE_STAGING = "staging-iphone-ios-260809-143022";

/** Shared label + control grid so the path fields share one nowrap label column. */
const settingsGrid = "grid grid-cols-[13.5rem_minmax(0,1fr)] items-center gap-x-3 gap-y-1";
const settingsLabel = "whitespace-nowrap text-[0.875rem] font-medium text-text";
const settingsHelp = "col-start-2 pl-2 text-[0.75rem] text-muted";

const FFMPEG_DEBOUNCE_MS = 300;

/** Example path shown under the Staging directory field. */
function stagingHelpExample(stagingDir: string, defaultDir: string): string {
  const trimmed = stagingDir.trim().replace(/[/\\]+$/, "");
  const defaultTrimmed = defaultDir.trim().replace(/[/\\]+$/, "");
  if (!trimmed || (defaultTrimmed && trimmed === defaultTrimmed)) {
    return `~/message-crate/${EXAMPLE_STAGING}`;
  }
  return `${trimmed}/${EXAMPLE_STAGING}`;
}

function persistFfmpegDir(dir: string): void {
  const trimmed = dir.trim();
  if (!trimmed) {
    removePref(FFMPEG_TOOLS_STORAGE_KEY);
    return;
  }
  writePref(FFMPEG_TOOLS_STORAGE_KEY, trimmed);
}

function ToolStatusRow({ name, path }: { name: "ffmpeg" | "ffprobe"; path: string | null }) {
  if (path) {
    const label = `Found ${name} - ${path}`;
    return (
      <li className="flex items-start gap-1.5 text-[0.75rem] text-text" aria-label={label}>
        <CheckIcon size={14} className="mt-0.5 shrink-0 text-ok" />
        <span>
          Found <code className="font-mono text-[0.7rem]">{name}</code>
          {" - "}
          <code className="font-mono text-[0.7rem]">{path}</code>
        </span>
      </li>
    );
  }
  const label = `${name} not found`;
  return (
    <li className="flex items-start gap-1.5 text-[0.75rem] text-text" aria-label={label}>
      <XIcon size={14} className="mt-0.5 shrink-0 text-danger" />
      <span>
        <code className="font-mono text-[0.7rem]">{name}</code> not found
      </span>
    </li>
  );
}

/** This app's own Build, shown on the System tab in the browser and the desktop app alike. */
function AppVersion() {
  return (
    <div>
      <h3 className={sectionHeading}>About</h3>
      <div className={settingsGrid}>
        <span className={settingsLabel}>Version</span>
        <span className="pl-2 font-mono text-[0.813rem] text-text">{APP_BUILD}</span>
      </div>
    </div>
  );
}

/**
 * The notice the GPL asks for. The desktop installer ships the Apple Messages
 * reader, a separate program under the GNU GPL v3, so whoever installed the
 * desktop app received a copy and must be able to find its license and the
 * source that matches it. The website ships no such program, so the browser
 * build never shows this.
 */
function ThirdPartySoftware() {
  const link = "text-accent";
  return (
    <div className="mt-8">
      <h3 className={sectionHeading}>Third-party software</h3>
      <p className="m-0 max-w-prose text-[0.875rem] text-text">
        Apple Messages are read by the Apple Messages reader (imessage-reader), a separate program
        installed beside this app. It is free software under the GNU General Public License, version
        3 or later, and its source is published with each release.
      </p>
      <p className="m-0 mt-1 text-[0.875rem]">
        <a href={readerSourceUrl(APP_BUILD)} target="_blank" rel="noopener" className={link}>
          Source
        </a>
        <span className="text-muted"> · </span>
        <a href={readerLicenseUrl(APP_BUILD)} target="_blank" rel="noopener" className={link}>
          License
        </a>
      </p>
    </div>
  );
}

/**
 * Where the Message Crate this app starts keeps everything. The folder is the
 * whole Message Crate, so it is what a person copies to back it up; the app
 * opens it rather than naming a path to find.
 */
function DataFolder() {
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState(getOpenToNetwork);
  const [server, setServer] = useState<LocalServerStatus | null>(null);
  const [networkError, setNetworkError] = useState<string | null>(null);

  const onOpenChange = (on: boolean) => {
    setOpen(on);
    setOpenToNetwork(on);
    setNetworkError(null);
    // While the app uses another Message Crate, the setting waits for the
    // next start of its own.
    if (!isOwnAddress(getBaseUrl())) return;
    setLocalServerOpenToNetwork(on).then(setServer, (caught: unknown) => {
      setNetworkError(caught instanceof Error ? caught.message : String(caught));
    });
  };
  // A Message Crate the app only found (Docker on this computer) listens
  // where it was told to, not where this setting says.
  const notOurs = server?.status === "ready" && !server.started_by_app;
  return (
    <div className="mt-8">
      <h3 className={sectionHeading}>Message Crate on this computer</h3>
      <p className="m-0 max-w-prose text-[0.875rem] text-text">
        The Message Crate this app starts keeps its database and attachments in one folder. A copy
        of that folder is a backup.
      </p>
      <button
        type="button"
        className="mt-2 rounded border border-border px-3 py-1.5 text-[0.875rem] text-text hover:bg-elevated"
        onClick={() => {
          setError(null);
          openDataFolder().catch((caught: unknown) => {
            setError(caught instanceof Error ? caught.message : String(caught));
          });
        }}
      >
        Open data folder
      </button>
      {error ? (
        <p className="m-0 mt-1 text-[0.75rem] text-danger" role="alert">
          {error}
        </p>
      ) : null}

      <Checkbox
        labelClassName="mt-5 flex items-start text-[0.875rem]"
        className="mt-[0.15rem]"
        checked={open}
        onChange={onOpenChange}
      >
        <span>
          Let other devices on this network connect
          <span className="mt-1 block max-w-prose text-[0.75rem] text-muted">
            A phone or another computer can then open this computer's address on port 8080, for as
            long as this app is open. The connection is plain HTTP, so anyone on the network can
            read what is sent, passwords included. Changing this restarts Message Crate once no
            import is running.
          </span>
        </span>
      </Checkbox>
      {notOurs ? (
        <p className="m-0 mt-1 max-w-prose text-[0.75rem] text-muted" role="status">
          This app did not start the Message Crate that is running on this computer, so the setting
          does not change it.
        </p>
      ) : null}
      {networkError ? (
        <p className="m-0 mt-1 text-[0.75rem] text-danger" role="alert">
          {networkError}
        </p>
      ) : null}
    </div>
  );
}

export function SystemSection() {
  const stagingId = useId();
  const ffmpegId = useId();
  const [ffmpegPath, setFfmpegPath] = useState("");
  const [stagingPath, setStagingPath] = useState("");
  const [defaultStagingPath, setDefaultStagingPath] = useState("");
  /** The Staging Directory the desktop process holds now. */
  const [savedStagingPath, setSavedStagingPath] = useState("");
  const [stagingError, setStagingError] = useState<string | null>(null);
  const [rememberPaths, setRememberPaths] = useState(false);
  const [probe, setProbe] = useState<FfmpegToolsProbe | null>(null);
  const ffmpegDebounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const ffmpegApplyGen = useRef(0);

  const runFfmpegApply = useCallback(async (dir: string) => {
    const gen = ++ffmpegApplyGen.current;
    const trimmed = dir.trim();
    try {
      if (!trimmed) {
        const result = await setFfmpegToolsDir(null);
        if (gen !== ffmpegApplyGen.current) return;
        persistFfmpegDir("");
        setProbe(result);
        return;
      }

      const probed = await probeFfmpegTools(trimmed);
      if (gen !== ffmpegApplyGen.current) return;
      setProbe(probed);
      if (!probed.ok) return;

      const applied = await setFfmpegToolsDir(trimmed);
      if (gen !== ffmpegApplyGen.current) return;
      setProbe(applied);
      if (applied.ok) persistFfmpegDir(trimmed);
    } catch {
      if (gen !== ffmpegApplyGen.current) return;
      setProbe({
        ok: false,
        ffmpeg_path: null,
        ffprobe_path: null,
        error: "Could not check ffmpeg tools",
      });
    }
  }, []);

  useEffect(() => {
    if (!isTauri()) return;

    setRememberPaths(getRememberImporterPaths());
    const storedFfmpeg = readPref(FFMPEG_TOOLS_STORAGE_KEY) || "";
    setFfmpegPath(storedFfmpeg);

    void (async () => {
      try {
        const staging = await invokeStagingRoot();
        setDefaultStagingPath(staging.defaultRoot);
        setSavedStagingPath(staging.root);
        setStagingPath(staging.root);
      } catch (caught: unknown) {
        setStagingError(caught instanceof Error ? caught.message : String(caught));
      }
      await runFfmpegApply(storedFfmpeg);
    })();

    return () => {
      if (ffmpegDebounceRef.current) clearTimeout(ffmpegDebounceRef.current);
    };
  }, [runFfmpegApply]);

  // The desktop process keeps the setting and decides what it takes: every
  // value typed is sent, and a refusal is shown as the desktop process gave
  // it, so the window holds no rule of its own. An empty value goes back to
  // the default. A run already staged keeps the folder it was made in; the
  // new setting applies to runs started after it.
  const stagingSaveGen = useRef(0);
  const onStagingPathChange = (next: string) => {
    setStagingPath(next);
    const gen = ++stagingSaveGen.current;
    invokeSetStagingRoot(next.trim()).then(
      (saved) => {
        if (gen !== stagingSaveGen.current) return;
        setSavedStagingPath(saved.root);
        setStagingError(null);
      },
      (caught: unknown) => {
        if (gen !== stagingSaveGen.current) return;
        setStagingError(`Not saved. ${caught instanceof Error ? caught.message : String(caught)}`);
      },
    );
  };

  // A value the desktop process refused stays in the field until it is left,
  // then the field shows the folder in use again.
  const onStagingPathBlur = () => {
    if (stagingError === null) return;
    stagingSaveGen.current += 1;
    setStagingPath(savedStagingPath);
    setStagingError(null);
  };

  const onFfmpegPathChange = (next: string) => {
    setFfmpegPath(next);
    if (ffmpegDebounceRef.current) clearTimeout(ffmpegDebounceRef.current);
    ffmpegDebounceRef.current = setTimeout(() => {
      void runFfmpegApply(next);
    }, FFMPEG_DEBOUNCE_MS);
  };

  if (!isTauri()) {
    return (
      <div>
        <AppVersion />
        <p className="m-0 mt-8 text-[0.875rem] text-muted">
          System settings (staging directory, ffmpeg tools, and remembered importer paths) are
          available in the desktop app.
        </p>
      </div>
    );
  }

  const helpExample = stagingHelpExample(stagingPath, defaultStagingPath);

  return (
    <div>
      <h3 className={sectionHeading}>Staging</h3>
      <div className={settingsGrid}>
        <label htmlFor={stagingId} className={settingsLabel}>
          Staging directory
        </label>
        <div>
          <PathPicker
            id={stagingId}
            value={stagingPath}
            onChange={onStagingPathChange}
            onBlur={onStagingPathBlur}
            directory
            placeholder={defaultStagingPath || "~/message-crate"}
          />
        </div>
        {stagingError ? (
          <p className="col-start-2 m-0 pl-2 text-[0.75rem] text-danger" role="alert">
            {stagingError}
          </p>
        ) : null}
        <p className={settingsHelp}>
          Temporary files for Import and Export are written here. For example {helpExample}
        </p>
      </div>

      <Checkbox
        labelClassName="mt-5 flex items-start text-[0.875rem]"
        className="mt-[0.15rem]"
        checked={rememberPaths}
        onChange={(on) => {
          setRememberPaths(on);
          setRememberImporterPaths(on);
        }}
      >
        <span>
          Remember importer paths
          <span className="mt-1 block text-[0.75rem] text-muted">
            When enabled, Import restores the last backup path for each import source.
          </span>
        </span>
      </Checkbox>

      <div className="mt-8">
        <h3 className={sectionHeading}>Media</h3>
        <div className={settingsGrid}>
          <label htmlFor={ffmpegId} className={settingsLabel}>
            ffmpeg directory
          </label>
          <div>
            <PathPicker
              id={ffmpegId}
              value={ffmpegPath}
              onChange={onFfmpegPathChange}
              directory
              placeholder="Uses system PATH by default"
            />
          </div>
          <p className={settingsHelp}>
            Folder must contain both ffmpeg and ffprobe. Leave blank to use system PATH.{" "}
            <a
              href="https://messagecrate.app/docs/user/features/messages/attachments-and-media/"
              target="_blank"
              rel="noopener"
              className="text-accent"
            >
              Install help
            </a>
          </p>
          {probe ? (
            <ul className={`${settingsHelp} mt-1 list-none space-y-1 p-0`}>
              <ToolStatusRow name="ffmpeg" path={probe.ffmpeg_path} />
              <ToolStatusRow name="ffprobe" path={probe.ffprobe_path} />
            </ul>
          ) : null}
        </div>
      </div>

      <DataFolder />

      <div className="mt-8">
        <AppVersion />
        <ThirdPartySoftware />
      </div>
    </div>
  );
}

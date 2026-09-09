import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { resolveStagingParent } from "./system-settings";
import type {
  AttachmentMediaMode,
  ExtractConfig,
  ExtractErrorEvent,
  ImportIssueEvent,
  ImportProgressEvent,
} from "./types";

/** Start extracting a phone backup on the desktop backend. */
export async function invokeExtract(config: ExtractConfig): Promise<void> {
  return invoke("extract", {
    args: {
      source: config.source,
      path: config.path,
      outputDir: config.output_dir,
      backupPassword: config.backup_password ?? null,
      attachmentMedia: config.attachment_media ?? null,
      mediaMaxResolution: config.media_max_resolution ?? null,
      mediaMaxFps: config.media_max_fps ?? null,
      mediaMinSize: config.media_min_size ?? null,
      obfuscate: config.obfuscate ?? null,
      ownerPhones: config.owner_phones ?? null,
      ownerEmails: config.owner_emails ?? null,
      timeZone: config.time_zone ?? null,
      attachmentRoot: config.attachment_root ?? null,
      appleContacts: config.apple_contacts ?? null,
      whatsappKey: config.whatsapp_key ?? null,
      whatsappWa: config.whatsapp_wa ?? null,
      whatsappMedia: config.whatsapp_media ?? null,
      whatsappDb: config.whatsapp_db ?? null,
      whatsappBusiness: config.whatsapp_business ?? null,
      resume: config.resume ?? null,
    },
  });
}

/** Ask the desktop backend to stop the job that is currently running. */
export async function invokeCancel(): Promise<void> {
  return invoke("cancel");
}

/**
 * Form fields shared by `summarize_staging` and `transcode_staging` — the
 * same media fields `ExtractConfig` carries, addressed at an already-staged
 * folder instead of a fresh backup.
 *
 * There is no `staging_root` field: the wrappers below resolve it themselves
 * via `resolveStagingParent`, the same source `openPathInExplorer`
 * uses, so no caller can pass a root that disagrees with the Rust-side
 * containment guard.
 */
export interface StagingConfig {
  staging_dir: string;
  attachment_media?: AttachmentMediaMode;
  media_max_resolution?: string;
  media_max_fps?: string;
  media_min_size?: string;
}

/**
 * Resolve the Staging Directory root every staging command must
 * check `staging_dir` against, throwing when it cannot be determined —
 * mirrors `openPathInExplorer`'s own resolution and error.
 */
async function resolveStagingRoot(): Promise<string> {
  const root = await resolveStagingParent();
  if (!root) {
    throw new Error("Could not determine the staging directory");
  }
  return root;
}

/** How a staged attachment is expected to land against the size limit. */
export type SizeVerdict =
  | "fits_as_is"
  | "likely_fits"
  | "may_grow"
  | "probably_too_big"
  | "cannot_process";

/** One attachment the user should see before approving. */
export interface AttachmentForecast {
  path: string;
  name: string;
  sizeBytes: number;
  estimateBytes: number;
  verdict: SizeVerdict;
}

/** How many attachments landed in each verdict. */
export interface VerdictCounts {
  fitsAsIs: number;
  likelyFits: number;
  mayGrow: number;
  probablyTooBig: number;
  cannotProcess: number;
}

/** What a staged folder holds, recomputed for the first approval gate. */
export interface StagingSummary {
  conversations: number;
  messages: number;
  contactIdentifiers: string[];
  attachments: number;
  attachmentBytes: number;
  verdictCounts: VerdictCounts;
  forecasts: AttachmentForecast[];
}

/** Recompute what a staged folder holds, for the first approval gate. */
export async function invokeSummarizeStaging(config: StagingConfig): Promise<StagingSummary> {
  const stagingRoot = await resolveStagingRoot();
  return invoke("summarize_staging", {
    args: {
      stagingDir: config.staging_dir,
      stagingRoot,
      attachmentMedia: config.attachment_media ?? null,
      mediaMaxResolution: config.media_max_resolution ?? null,
      mediaMaxFps: config.media_max_fps ?? null,
      mediaMinSize: config.media_min_size ?? null,
    },
  });
}

/**
 * Run the convert/compress pass over a staged folder, after the first gate
 * approves it. Reports through the `extract:*` events like every other long
 * job, so `runTauriJob` drives it exactly as it drives extract and push.
 */
export async function invokeTranscodeStaging(config: StagingConfig): Promise<void> {
  const stagingRoot = await resolveStagingRoot();
  return invoke("transcode_staging", {
    args: {
      stagingDir: config.staging_dir,
      stagingRoot,
      attachmentMedia: config.attachment_media ?? null,
      mediaMaxResolution: config.media_max_resolution ?? null,
      mediaMaxFps: config.media_max_fps ?? null,
      mediaMinSize: config.media_min_size ?? null,
    },
  });
}

/**
 * Delete a staging folder — the decline path's terminal action: closing an
 * approval gate without approving deletes the folder outright.
 */
export async function invokeDeleteStaging(config: { staging_dir: string }): Promise<void> {
  const stagingRoot = await resolveStagingRoot();
  return invoke("delete_staging", {
    args: {
      stagingDir: config.staging_dir,
      stagingRoot,
    },
  });
}

export interface PushConfig {
  base_url: string;
  username: string;
  key: string;
  input_dir: string;
  mode: string;
  force: boolean;
  continue_on_error: boolean;
  skip_attachments: boolean;
  trust_export: boolean;
  import_id?: number;
}

export interface PushFinishedReport {
  ok: boolean;
  /** Older field: messages counted in successful HTTP requests. */
  messages: number;
  messages_attempted: number;
  messages_inserted: number;
  messages_deduped: number;
  messages_failed: number;
  assets_uploaded: number;
  assets_bytes: number;
  conversations_ok: number;
  conversations_total: number;
  conversations_failed: number;
  conversations_skipped: number;
  results: Array<{
    file: string;
    status: string;
    error?: string;
    messages: number;
    attachments: number;
  }>;
}

/**
 * What `transcode_staging`'s job did, from its `extract:finished` payload.
 * `TranscodeReport` (`ir-format`) has no serde derive: `transcode_staging`
 * (`src-tauri/src/commands/staging.rs`) hand-builds the payload with these
 * fields flat at the top level, alongside `summary` — not nested under a
 * `report` key — so this mirrors the wire shape exactly, snake_case included.
 */
export interface TranscodeFinishedReport {
  converted: number;
  skipped: number;
  too_large: number;
  failed: number;
  missing: number;
  repointed: number;
  bytes_before: number;
  bytes_after: number;
}

export interface TauriJobResult {
  summary: string;
  report?: PushFinishedReport;
  extraction?: {
    files_parsed: number;
    messages_parsed: number;
  };
  transcode?: TranscodeFinishedReport;
}

/** Upload extracted conversations to a vault server. */
export async function invokePush(config: PushConfig): Promise<void> {
  return invoke("push", {
    args: {
      baseUrl: config.base_url,
      username: config.username,
      key: config.key,
      inputDir: config.input_dir,
      mode: config.mode,
      force: config.force,
      continueOnError: config.continue_on_error,
      skipAttachments: config.skip_attachments,
      trustExport: config.trust_export,
      importId: config.import_id ?? null,
    },
  });
}

export interface PullConfig {
  base_url: string;
  username: string;
  key: string;
  out_dir: string;
  query: string;
  skip_attachments: boolean;
}

/** Download conversations from a vault server into a folder. */
export async function invokePull(config: PullConfig): Promise<void> {
  return invoke("pull", {
    args: {
      baseUrl: config.base_url,
      username: config.username,
      key: config.key,
      outDir: config.out_dir,
      query: config.query,
      skipAttachments: config.skip_attachments,
    },
  });
}

/**
 * File formats `message-reexport` can write, in the order the Export screen
 * offers them. The ids are the strings the `format` command parses
 * (`src-tauri/src/commands/format.rs`); anything else is rejected there.
 */
export const EXPORT_FORMATS = [
  { id: "jsonl", label: "JSON Lines (.jsonl)" },
  { id: "json", label: "JSON (.json)" },
  { id: "csv", label: "CSV (.csv)" },
  { id: "eml", label: "EML (one file per message)" },
  { id: "mbox", label: "MBOX (.mbox)" },
  { id: "xml", label: "Android XML (smses.xml)" },
] as const;

/** Id of a format the Export screen can write. */
export type ExportFormat = (typeof EXPORT_FORMATS)[number]["id"];

/**
 * Rewrite an export folder into another format.
 *
 * `input_dir` and `output_dir` must differ: `message-reexport` canonicalizes
 * both and refuses to write into its own input.
 */
export async function invokeFormat(config: {
  input_dir: string;
  output_dir: string;
  output_format: ExportFormat;
}): Promise<void> {
  return invoke("format", {
    inputDir: config.input_dir,
    outputDir: config.output_dir,
    outputFormat: config.output_format,
  });
}

export interface FfmpegToolsProbe {
  ok: boolean;
  ffmpeg_path: string | null;
  ffprobe_path: string | null;
  error: string | null;
}

/** Check whether ffmpeg and ffprobe are available at this folder. */
export async function probeFfmpegTools(dir: string | null): Promise<FfmpegToolsProbe> {
  return invoke("probe_ffmpeg_tools", { dir });
}

/** Save the ffmpeg tools folder and check that the tools are there. */
export async function setFfmpegToolsDir(dir: string | null): Promise<FfmpegToolsProbe> {
  return invoke("set_ffmpeg_tools_dir", { dir });
}

export interface HomeDirInfo {
  path: string;
  os: string;
}

/** User home folder and operating system name from the desktop backend. */
export async function invokeHomeDir(): Promise<HomeDirInfo> {
  return invoke("home_dir");
}

export interface PathStat {
  exists: boolean;
  isFile: boolean;
  isDirectory: boolean;
  sizeBytes: number;
  modifiedUnixMs: number | null;
}

/** Whether a path exists and whether it is a file or directory. */
export async function invokePathStat(path: string): Promise<PathStat> {
  return invoke("path_stat", { path });
}

/** Whether an iOS backup folder is encrypted, or null when unknown. */
export async function invokeIosBackupEncrypted(path: string): Promise<boolean | null> {
  return invoke("ios_backup_encrypted", { path });
}

/**
 * Addresses an iMessage backup's device sent from. The desktop opens the
 * source the same way the extractor will, so a source it cannot read fails
 * here with the message the extractor would give.
 */
export async function invokeImessageBackupIdentities(args: {
  path: string;
  ios: boolean;
  backupPassword: string;
}): Promise<string[]> {
  return invoke("imessage_backup_identities", {
    path: args.path,
    ios: args.ios,
    backupPassword: args.backupPassword.trim() === "" ? null : args.backupPassword,
  });
}

/**
 * Listen for job events from the desktop backend (log lines, progress, errors).
 * Returns one function that removes every listener.
 */
export function onExtractEvents(callbacks: {
  onLog: (line: string) => void;
  onProgress?: (event: ImportProgressEvent) => void;
  onIssue?: (event: ImportIssueEvent) => void;
  onFinished: (summary: string) => void;
  onError: (err: ExtractErrorEvent) => void;
}): Promise<UnlistenFn> {
  return Promise.all([
    listen<string>("extract:log", (e) => callbacks.onLog(e.payload)),
    listen<ImportProgressEvent>("extract:progress", (e) => callbacks.onProgress?.(e.payload)),
    listen<ImportIssueEvent>("extract:issue", (e) => callbacks.onIssue?.(e.payload)),
    listen<string>("extract:finished", (e) => callbacks.onFinished(e.payload)),
    listen<ExtractErrorEvent>("extract:error", (e) => callbacks.onError(e.payload)),
  ]).then((unlisteners) => {
    return () => {
      for (const u of unlisteners) {
        u();
      }
    };
  });
}

/**
 * Run a desktop job and wait until it finishes.
 * Extract and push return as soon as the background thread starts, so callers
 * must use this instead of awaiting the invoke call alone.
 */
export async function awaitTauriJob(
  invokeFn: () => Promise<void>,
  onLog?: (line: string) => void,
  onProgress?: (event: ImportProgressEvent) => void,
  onIssue?: (event: ImportIssueEvent) => void,
): Promise<TauriJobResult> {
  let unlisten: UnlistenFn | undefined;
  try {
    return await new Promise<TauriJobResult>((resolve, reject) => {
      void (async () => {
        try {
          unlisten = await onExtractEvents({
            onLog: (line) => onLog?.(line),
            onProgress,
            onIssue,
            onFinished: (summary) => resolve(parseTauriJobResult(summary)),
            onError: (err) => reject(new Error(err.user_message ?? err.detail)),
          });
          await invokeFn();
        } catch (e: unknown) {
          reject(e instanceof Error ? e : new Error(String(e)));
        }
      })();
    });
  } finally {
    unlisten?.();
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isPushFinishedReport(value: unknown): value is PushFinishedReport {
  if (!isRecord(value)) return false;
  return (
    typeof value.ok === "boolean" &&
    typeof value.messages === "number" &&
    typeof value.messages_attempted === "number" &&
    typeof value.messages_inserted === "number" &&
    typeof value.messages_deduped === "number" &&
    typeof value.messages_failed === "number" &&
    typeof value.assets_uploaded === "number" &&
    typeof value.assets_bytes === "number" &&
    typeof value.conversations_ok === "number" &&
    typeof value.conversations_total === "number" &&
    typeof value.conversations_failed === "number" &&
    typeof value.conversations_skipped === "number"
  );
}

function isTranscodeFinishedReport(value: unknown): value is TranscodeFinishedReport {
  if (!isRecord(value)) return false;
  return (
    typeof value.converted === "number" &&
    typeof value.skipped === "number" &&
    typeof value.too_large === "number" &&
    typeof value.failed === "number" &&
    typeof value.missing === "number" &&
    typeof value.repointed === "number" &&
    typeof value.bytes_before === "number" &&
    typeof value.bytes_after === "number"
  );
}

/** Turn a finished-job summary string into a structured result when it is JSON. */
export function parseTauriJobResult(summary: string): TauriJobResult {
  try {
    const parsed: unknown = JSON.parse(summary);
    if (!isRecord(parsed)) return { summary };

    if (
      typeof parsed.summary === "string" &&
      typeof parsed.files_parsed === "number" &&
      typeof parsed.messages_parsed === "number"
    ) {
      return {
        summary: parsed.summary,
        extraction: {
          files_parsed: parsed.files_parsed,
          messages_parsed: parsed.messages_parsed,
        },
      };
    }

    const summaryText = typeof parsed.summary === "string" ? parsed.summary : summary;

    if (isPushFinishedReport(parsed)) {
      return {
        summary: summaryText,
        report: parsed,
      };
    }

    if (isTranscodeFinishedReport(parsed)) {
      // Picked field by field, not spread: `parsed` also carries the raw
      // `summary` string this same object holds the report fields
      // alongside, which isn't part of `TranscodeFinishedReport`.
      return {
        summary: summaryText,
        transcode: {
          converted: parsed.converted,
          skipped: parsed.skipped,
          too_large: parsed.too_large,
          failed: parsed.failed,
          missing: parsed.missing,
          repointed: parsed.repointed,
          bytes_before: parsed.bytes_before,
          bytes_after: parsed.bytes_after,
        },
      };
    }
  } catch {
    // Extract jobs send a plain sentence, not JSON.
  }
  return { summary };
}

import { useEffect, useRef, useState } from "react";
import { isAndroidSmsSource, needsOwnerEmails, splitEmails } from "../lib/androidSmsSources";
import {
  type IdentityService,
  identityOnProfile,
  parseSourceIdentities,
} from "../lib/backupIdentity";
import { getDeviceId } from "../lib/deviceId";
import {
  emptyImessagePathStats,
  IMESSAGE_DEFAULT_METHOD,
  IMESSAGE_SOURCE_ID,
  type ImessageMethodId,
  imessageStatsForMethod,
  isImessageMethod,
  macMessagesDbPath,
  type PathStat,
  shouldPrefillMacMessagesDb,
} from "../lib/imessageImport";
import { discardImportSession, getActiveImportSession } from "../lib/importSession";
import {
  getImporterPath,
  getRememberImporterPaths,
  loadRememberedImportPaths,
  setImporterExtraPath,
  setImporterPath,
} from "../lib/system-settings";
import {
  invokeDeleteStaging,
  invokeHomeDir,
  invokeIosBackupEncrypted,
  invokePathStat,
} from "../lib/tauri";
import { isTauri } from "../lib/tauri-check";
import type { AttachmentMediaMode } from "../lib/types";
import {
  useAccountProfile,
  useFetchAccountProfile,
  useUpdateAccountProfile,
} from "../lib/useAccountProfile";
import { unmatchedHandles } from "../lib/vaultApi";
import {
  emptyWhatsappPathStats,
  isWhatsappMethod,
  WHATSAPP_CRYPT_NAMES,
  WHATSAPP_DEFAULT_METHOD,
  WHATSAPP_SOURCE_ID,
  type WhatsappMethodId,
} from "../lib/whatsappImport";
import BackupIdentityList from "./import/BackupIdentityList";
import BackupIdentityStopScreen from "./import/BackupIdentityStopScreen";
import { restoreFormFromSnapshot } from "./import/formSnapshot";
import GateOneScreen from "./import/GateOneScreen";
import GateTwoScreen from "./import/GateTwoScreen";
import ImportFormFields from "./import/ImportFormFields";
import ImportProgressView from "./import/ImportProgressView";
import ResumeImportPanel from "./import/ResumeImportPanel";
import {
  checkSourceFingerprint,
  type FolderCheck,
  type ResumeDecision,
  resumeDecisionFor,
} from "./import/resumeDecision";
import { parseStoredStagingSummary, useImportJob } from "./import/useImportJob";

const DEFAULT_SOURCE = IMESSAGE_DEFAULT_METHOD;
const PATH_PROBE_DEBOUNCE_MS = 200;
/** The server's own cap on one `/v1/contacts/unmatched-handles` request (`MAX_MATCH_IDENTIFIERS`,
 * `crates/vault/server/src/contacts_api.rs`) — the client batches to it rather than
 * discovering the limit from a 400. */
const MAX_MATCH_IDENTIFIERS = 500;

/** Nothing to decide -- the form renders. The one spelling of "no resume". */
const NO_RESUME: ResumeDecision = { kind: "none", session: null };

function mapPathStat(raw: { exists: boolean; isFile: boolean; isDirectory: boolean }): PathStat {
  return {
    exists: raw.exists,
    isFile: raw.isFile,
    isDirectory: raw.isDirectory,
  };
}

async function probePath(path: string): Promise<PathStat | null> {
  const trimmed = path.trim();
  if (trimmed === "") return null;
  try {
    return mapPathStat(await invokePathStat(trimmed));
  } catch {
    return { exists: false, isFile: false, isDirectory: false };
  }
}

/**
 * Whether a session's staging folder is still on disk. A stat that fails
 * outright is "unknown", not "missing": an IPC error says nothing about the
 * folder, and reading it as gone would offer to discard staged work that
 * may well still be there.
 */
async function stagingFolderCheck(stagingDir: string): Promise<FolderCheck> {
  try {
    const stat = await invokePathStat(stagingDir);
    return stat.exists && stat.isDirectory ? "present" : "missing";
  } catch {
    return "unknown";
  }
}

export default function ImportScreen() {
  const fetchAccountProfile = useFetchAccountProfile();
  const {
    phase,
    steps,
    running,
    summaryView,
    stagingDir,
    gateSummary,
    gateDelta,
    gateAttachmentMedia,
    mediaToolsMissing,
    mediaPartiallyRan,
    resumeError,
    sourceIdentities,
    computingSummary,
    completionText,
    startImport,
    approveGate,
    declineGate,
    resumeAtGate,
    cancel,
    returnToForm,
    continueAfterIdentityStop,
    cancelIdentityStop,
  } = useImportJob();

  /** Null while the lookup hasn't finished (or failed) for the summary currently shown. */
  const [unknownContacts, setUnknownContacts] = useState<number | null>(null);

  const { profile } = useAccountProfile();
  const updateProfile = useUpdateAccountProfile();
  const identityProfile = profile ? { phones: profile.phones, emails: profile.emails } : null;
  const identityAddBusy = updateProfile.isPending;
  const [identityAddError, setIdentityAddError] = useState<string | null>(null);

  /** Link one backup address onto the profile; the marks re-derive from the
   * updated profile, so a claimed address resolves a mismatch in place.
   * Never rejects: a failed add (or a 200 that didn't actually add it) is
   * caught here and turned into `identityAddError` rather than an unhandled
   * rejection through the fire-and-forget `void onAdd(...)` call in
   * BackupIdentityList/BackupIdentityStopScreen. */
  const addIdentityToProfile = async (value: string, service: IdentityService): Promise<void> => {
    setIdentityAddError(null);
    try {
      const updated = await updateProfile.mutateAsync({
        handles: [{ handle: value, service }],
      });
      if (!identityOnProfile(value, updated)) {
        throw new Error("no-op add");
      }
    } catch {
      setIdentityAddError("The vault didn't add that address.");
    }
  };

  const [source, setSource] = useState(DEFAULT_SOURCE);
  const [backupPath, setBackupPath] = useState(() =>
    getRememberImporterPaths() ? getImporterPath(DEFAULT_SOURCE) : "",
  );
  const [attachmentRoot, setAttachmentRoot] = useState("");
  const [appleContacts, setAppleContacts] = useState("");
  const [pathStats, setPathStats] = useState(emptyImessagePathStats);
  const [whatsappKey, setWhatsappKey] = useState("");
  const [showWhatsappKey, setShowWhatsappKey] = useState(false);
  const [whatsappWa, setWhatsappWa] = useState("");
  const [whatsappMedia, setWhatsappMedia] = useState("");
  const [whatsappDb, setWhatsappDb] = useState("");
  const [whatsappBusiness, setWhatsappBusiness] = useState(false);
  const [whatsappStats, setWhatsappStats] = useState(emptyWhatsappPathStats);
  const [backupPassword, setBackupPassword] = useState("");
  const [showBackupPassword, setShowBackupPassword] = useState(false);
  const [attachmentMedia, setAttachmentMedia] = useState<AttachmentMediaMode>("copy");
  const [maxResolution, setMaxResolution] = useState("720p");
  const [maxFps, setMaxFps] = useState("30");
  const [minSizeMb, setMinSizeMb] = useState("20");
  const [ownerPhones, setOwnerPhones] = useState<string[]>([]);
  /** Owner email addresses as typed; split into a list when the import starts. */
  const [ownerEmails, setOwnerEmails] = useState("");
  const [formatOpen, setFormatOpen] = useState(true);
  const [processingOpen, setProcessingOpen] = useState(false);
  const [force, setForce] = useState(false);
  const [obfuscate, setObfuscate] = useState(false);
  /** Profile phones after SBR fetch; empty until ready (or after a failed fetch). */
  const [profilePhones, setProfilePhones] = useState<string[]>([]);
  const [profilePhonesReady, setProfilePhonesReady] = useState(false);
  const [profilePhonesError, setProfilePhonesError] = useState(false);
  const ownerPhonesSeededRef = useRef(false);
  const ownerEmailsSeededRef = useRef(false);
  const lastImessageMethodRef = useRef<ImessageMethodId>(IMESSAGE_DEFAULT_METHOD);
  const lastWhatsappMethodRef = useRef<WhatsappMethodId>(WHATSAPP_DEFAULT_METHOD);
  const sourceChangeGenRef = useRef(0);

  const [resume, setResume] = useState<ResumeDecision>(NO_RESUME);
  const [resumeChecked, setResumeChecked] = useState(false);
  const discardingRef = useRef(false);
  const resumingRef = useRef(false);

  /**
   * Ask the vault what session is open, on mount and on every return to the
   * form.
   *
   * Re-checking matters because the vault can hold a session the screen has
   * already forgotten: a swallowed final /complete, or a restart whose
   * discard failed before the create 409'd. Without it, Back lands on a
   * blank form whose Import button 409s until the route is remounted.
   *
   * `phase` is the only dependency and nothing here writes it, so this
   * cannot loop; the early return keeps it from running against a session
   * an import is currently using.
   */
  useEffect(() => {
    if (phase !== "form") return;
    let cancelled = false;
    void (async () => {
      try {
        const session = await getActiveImportSession();
        const folder = session?.staging_dir
          ? await stagingFolderCheck(session.staging_dir)
          : "missing";
        // Only a resume of the copy consults this; every later stage works
        // from the staged folder rather than the backup. The full stat, not
        // `probePath`'s narrowed one: the comparison needs the size and
        // modified time.
        const sourceStat = session?.source_fingerprint?.path
          ? await invokePathStat(session.source_fingerprint.path).catch(() => null)
          : null;
        // A resume or discard that started while this was in flight owns
        // the decision -- a stale answer must not put the panel back.
        if (!cancelled && !resumingRef.current && !discardingRef.current) {
          setResume(
            resumeDecisionFor({
              session,
              deviceId: getDeviceId(),
              folder,
              fingerprint: checkSourceFingerprint(session?.source_fingerprint ?? null, sourceStat),
            }),
          );
        }
      } catch {
        // A vault that cannot answer is not a reason to block the form.
        if (!cancelled && !resumingRef.current && !discardingRef.current) setResume(NO_RESUME);
      } finally {
        // Only the first check gates what renders; a later one must not
        // blank the form while it runs.
        if (!cancelled) setResumeChecked(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [phase]);

  /**
   * Ask the vault which of Gate 1's contact identifiers this account already
   * has, once per summary shown at Gate 1 — batched at the server's own cap
   * so a large import doesn't send an oversized request. A failed batch
   * leaves the count unknown rather than blocking the gate (decision: the
   * "new to your vault" clause is a nicety, not a requirement).
   */
  useEffect(() => {
    if (phase !== "gate_1" || !gateSummary) return;
    let cancelled = false;
    setUnknownContacts(null);
    void (async () => {
      const identifiers = gateSummary.contactIdentifiers;
      let total = 0;
      try {
        for (let i = 0; i < identifiers.length; i += MAX_MATCH_IDENTIFIERS) {
          const batch = identifiers.slice(i, i + MAX_MATCH_IDENTIFIERS);
          const res = await unmatchedHandles({ identifiers: batch });
          total += res.unknown.length;
        }
        if (!cancelled) setUnknownContacts(total);
      } catch {
        if (!cancelled) setUnknownContacts(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [phase, gateSummary]);

  /** Populate the visible form from a resumed or restarted session's settings. */
  function applyRestoredFormState(restored: ReturnType<typeof restoreFormFromSnapshot>): void {
    if (!restored) return;
    setSource(restored.source);
    setBackupPath(restored.backupPath);
    setBackupPassword("");
    setAttachmentRoot(restored.attachmentRoot);
    setAppleContacts(restored.appleContacts);
    setWhatsappKey("");
    setWhatsappWa(restored.whatsappWa);
    setWhatsappMedia(restored.whatsappMedia);
    setWhatsappDb(restored.whatsappDb);
    setWhatsappBusiness(restored.whatsappBusiness);
    setAttachmentMedia(restored.attachmentMedia);
    setMaxResolution(restored.maxResolution);
    setMaxFps(restored.maxFps);
    setMinSizeMb(restored.minSizeMb);
    // Restoring settings counts as seeding: the SBR profile-phones effect
    // must not overwrite what was just restored.
    ownerPhonesSeededRef.current = true;
    setOwnerPhones(restored.ownerPhones);
    ownerEmailsSeededRef.current = true;
    setOwnerEmails(restored.ownerEmails.join(", "));
    setForce(restored.force);
    setObfuscate(restored.obfuscate);
  }

  async function handleDiscardResume(): Promise<void> {
    const session = resume.session;
    if (!session || discardingRef.current || resumingRef.current) return;
    discardingRef.current = true;
    try {
      // declineGate already deletes the staging folder on decline (decision
      // 16); a panel discard is the same operation reached through a
      // different button, so it must not orphan a multi-GB folder. Both
      // halves run regardless of the other's outcome, the same
      // `Promise.allSettled` shape `declineGate` uses. Never touch disk for
      // another device's session -- its files are staged there, not here --
      // the same `device_id` check `resumeDecisionFor` uses to route to
      // `other_device` in the first place. A session with no recorded
      // device is treated as this install's, matching that check too.
      const thisDevice = !session.device_id || session.device_id === getDeviceId();
      await Promise.allSettled([
        discardImportSession(session.id),
        thisDevice && session.staging_dir
          ? invokeDeleteStaging({ staging_dir: session.staging_dir })
          : Promise.resolve(),
      ]);
    } catch {
      // Best effort -- the panel drops to the form either way; if the
      // session is still live server-side, the next visit shows it again.
    } finally {
      discardingRef.current = false;
      setResume(NO_RESUME);
    }
  }

  async function handleResumeAction(): Promise<void> {
    if (resume.kind === "none" || !resume.session) return;
    // The panel deliberately stays mounted across the discard round trip
    // below, so without this a second click would run two discards, two
    // startImport calls (the second 409s), and two extracts racing one set
    // of screen state.
    if (resumingRef.current || discardingRef.current) return;
    resumingRef.current = true;
    try {
      const session = resume.session;
      const restoredForm = restoreFormFromSnapshot(session.form);
      if (!restoredForm) {
        // The staging folder is present -- the decision only reached here
        // because it is -- so folder_missing's copy would be false. This
        // kind exists solely for this screen to construct.
        setResume({ kind: "settings_unreadable", session });
        return;
      }
      applyRestoredFormState(restoredForm);

      if (resume.kind === "resume_push") {
        if (!session.staging_dir) return; // resumeDecisionFor guarantees this; defensive only.
        setResume(NO_RESUME);
        await startImport(restoredForm, {
          sessionId: session.id,
          stagingDir: session.staging_dir,
          // Without this, a resumed push has no plan to diff its expected
          // omissions against, which demotes an honest `completed` verdict
          // to `completed_with_issues` for exactly the interrupted-and-
          // resumed case. Undefined when the stored summary is missing or
          // unparsable — startImport/runPush already tolerate that.
          approved: parseStoredStagingSummary(session.summary),
        });
        return;
      }

      if (resume.kind === "resume_gate" || resume.kind === "resume_media") {
        if (!session.staging_dir) return; // resumeDecisionFor guarantees this; defensive only.
        setResume(NO_RESUME);
        await resumeAtGate(session, restoredForm);
        return;
      }

      if (resume.kind === "resume_write") {
        if (!session.staging_dir) return; // resumeDecisionFor guarantees this; defensive only.
        setResume(NO_RESUME);
        await startImport(restoredForm, undefined, {
          sessionId: session.id,
          stagingDir: session.staging_dir,
          // The write is resumed rather than re-probed, so Gate 1's identity
          // section has to come from what was recorded on the session at
          // creation rather than a fresh read of the backup.
          identities: parseSourceIdentities(session.source_identities),
        });
        return;
      }

      // Restart: a fresh extract writes into a new staging folder, and the
      // vault allows only one live session per account, so give up the old
      // one before starting the new run. setResume stays put until right
      // before startImport, so the panel (not a blank form) covers the
      // discard round trip. The old folder goes with the session: nothing
      // will ever reach it again, and it can be multiple gigabytes.
      const thisDevice = !session.device_id || session.device_id === getDeviceId();
      try {
        await Promise.allSettled([
          discardImportSession(session.id),
          thisDevice && session.staging_dir
            ? invokeDeleteStaging({ staging_dir: session.staging_dir })
            : Promise.resolve(),
        ]);
      } catch {
        // Best effort -- if the vault is unreachable the create call below
        // surfaces its own error the same as any other failed import start.
      }
      setResume(NO_RESUME);
      await startImport(restoredForm);
    } finally {
      resumingRef.current = false;
    }
  }

  useEffect(() => {
    if (!isAndroidSmsSource(source)) {
      setProfilePhones([]);
      setProfilePhonesReady(false);
      setProfilePhonesError(false);
      ownerPhonesSeededRef.current = false;
      ownerEmailsSeededRef.current = false;
      return;
    }
    const wantsEmails = needsOwnerEmails(source);
    let cancelled = false;
    setProfilePhonesReady(false);
    setProfilePhonesError(false);
    void (async () => {
      try {
        const profile = await fetchAccountProfile();
        if (cancelled) return;
        if (!profile) throw new Error("profile unavailable");
        setProfilePhones([...profile.phones]);
        setProfilePhonesError(false);
        setProfilePhonesReady(true);
        if (wantsEmails && profile.emails.length > 0 && !ownerEmailsSeededRef.current) {
          setOwnerEmails((current) => {
            if (current.trim().length > 0) return current;
            ownerEmailsSeededRef.current = true;
            return profile.emails.join(", ");
          });
        }
        if (profile.phones.length === 0 || ownerPhonesSeededRef.current) return;
        setOwnerPhones((current) => {
          if (current.length > 0) return current;
          ownerPhonesSeededRef.current = true;
          return [...profile.phones];
        });
      } catch {
        if (!cancelled) {
          setProfilePhones([]);
          setProfilePhonesError(true);
          setProfilePhonesReady(true);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [source, fetchAccountProfile]);

  useEffect(() => {
    return () => {
      sourceChangeGenRef.current += 1;
    };
  }, []);

  useEffect(() => {
    if (!isTauri() || !isImessageMethod(source)) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void (async () => {
        const [backup, attachment, contacts] = await Promise.all([
          probePath(backupPath),
          probePath(attachmentRoot),
          probePath(appleContacts),
        ]);
        let backupEncrypted: boolean | null = null;
        if (source === "imessage-ios" && backup?.exists && backup.isDirectory) {
          try {
            backupEncrypted = await invokeIosBackupEncrypted(backupPath.trim());
          } catch {
            backupEncrypted = null;
          }
        }
        if (cancelled) return;
        const next = {
          backup,
          attachmentRoot: attachment,
          appleContacts: contacts,
          backupEncrypted,
        };
        setPathStats(imessageStatsForMethod(source, next));
      })();
    }, PATH_PROBE_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [source, backupPath, attachmentRoot, appleContacts]);

  useEffect(() => {
    if (!isTauri() || !isWhatsappMethod(source)) return;
    setWhatsappStats(emptyWhatsappPathStats());
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void (async () => {
        const root = backupPath.trim();
        const [backup, contactsDb, media, db, msgstore, cryptHits] = await Promise.all([
          probePath(backupPath),
          probePath(whatsappWa),
          probePath(whatsappMedia),
          probePath(whatsappDb),
          probePath(root ? `${root}/msgstore.db` : ""),
          Promise.all(
            WHATSAPP_CRYPT_NAMES.map(async (name) => {
              const stat = await probePath(root ? `${root}/${name}` : "");
              return stat?.exists && stat.isFile ? name : null;
            }),
          ),
        ]);
        if (cancelled) return;
        const cryptName = cryptHits.find((name) => name !== null) ?? null;
        setWhatsappStats({
          backup,
          contactsDb,
          media,
          db,
          hasMsgstoreDb: Boolean(msgstore?.exists && msgstore.isFile),
          cryptName,
        });
      })();
    }, PATH_PROBE_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [source, backupPath, whatsappWa, whatsappMedia, whatsappDb]);

  function applyRememberedPaths(nextSource: string): string {
    const loaded = loadRememberedImportPaths(nextSource);
    setBackupPath(loaded.backupPath);
    if (isImessageMethod(nextSource)) {
      setAttachmentRoot(loaded.attachmentRoot);
      setAppleContacts(loaded.appleContacts);
      setWhatsappWa("");
      setWhatsappMedia("");
      setWhatsappDb("");
    } else if (isWhatsappMethod(nextSource)) {
      setAttachmentRoot("");
      setAppleContacts("");
      setWhatsappWa(loaded.whatsappWa);
      setWhatsappMedia(loaded.whatsappMedia);
      setWhatsappDb(loaded.whatsappDb);
    } else {
      setAttachmentRoot("");
      setAppleContacts("");
      setWhatsappWa("");
      setWhatsappMedia("");
      setWhatsappDb("");
    }
    return loaded.backupPath;
  }

  function handleSourceChange(next: string): void {
    const resolved =
      next === IMESSAGE_SOURCE_ID
        ? lastImessageMethodRef.current
        : next === WHATSAPP_SOURCE_ID
          ? lastWhatsappMethodRef.current
          : next;
    const gen = ++sourceChangeGenRef.current;
    setSource(resolved);
    if (isImessageMethod(resolved)) lastImessageMethodRef.current = resolved;
    if (isWhatsappMethod(resolved)) lastWhatsappMethodRef.current = resolved;
    setPathStats(emptyImessagePathStats());
    setWhatsappStats(emptyWhatsappPathStats());
    if (resolved !== "whatsapp-ios") setWhatsappBusiness(false);
    const loadedBackup = applyRememberedPaths(resolved);

    if (resolved !== "imessage-macos" || loadedBackup.trim() !== "" || !isTauri()) {
      return;
    }

    void (async () => {
      try {
        const home = await invokeHomeDir();
        if (gen !== sourceChangeGenRef.current) return;
        if (home.os !== "macos") return;
        const chatDb = macMessagesDbPath(home.path);
        if (chatDb === "") return;
        const stat = mapPathStat(await invokePathStat(chatDb));
        if (gen !== sourceChangeGenRef.current) return;
        const prefill = shouldPrefillMacMessagesDb({
          os: home.os,
          homeDir: home.path,
          chatDbExists: stat.exists && stat.isFile,
          rememberedPath: loadedBackup,
        });
        if (prefill === "") return;
        setBackupPath(prefill);
        if (getRememberImporterPaths()) setImporterPath(resolved, prefill);
      } catch {
        // Home directory and path checks are best-effort on Mac only.
      }
    })();
  }

  const updateBackupPath = (path: string) => {
    setBackupPath(path);
    if (getRememberImporterPaths()) setImporterPath(source, path);
  };

  const updateAttachmentRoot = (path: string) => {
    setAttachmentRoot(path);
    if (getRememberImporterPaths() && isImessageMethod(source)) {
      setImporterExtraPath(source, "attachmentRoot", path);
    }
  };

  const updateAppleContacts = (path: string) => {
    setAppleContacts(path);
    if (getRememberImporterPaths() && isImessageMethod(source)) {
      setImporterExtraPath(source, "appleContacts", path);
    }
  };

  const updateWhatsappWa = (path: string) => {
    setWhatsappWa(path);
    if (getRememberImporterPaths() && isWhatsappMethod(source)) {
      setImporterExtraPath(source, "whatsappWa", path);
    }
  };

  const updateWhatsappMedia = (path: string) => {
    setWhatsappMedia(path);
    if (getRememberImporterPaths() && isWhatsappMethod(source)) {
      setImporterExtraPath(source, "whatsappMedia", path);
    }
  };

  const updateWhatsappDb = (path: string) => {
    setWhatsappDb(path);
    if (getRememberImporterPaths() && isWhatsappMethod(source)) {
      setImporterExtraPath(source, "whatsappDb", path);
    }
  };

  const isAndroidSms = isAndroidSmsSource(source);

  return (
    <div className={`min-w-0 p-6 ${phase === "form" ? "max-w-[640px]" : "max-w-5xl"}`}>
      {phase === "form" && resumeChecked && resume.kind === "none" && (
        <ImportFormFields
          source={source}
          onSourceChange={handleSourceChange}
          backupPath={backupPath}
          onBackupPathChange={updateBackupPath}
          backupPassword={backupPassword}
          onBackupPasswordChange={setBackupPassword}
          showBackupPassword={showBackupPassword}
          onToggleBackupPassword={() => setShowBackupPassword((v) => !v)}
          attachmentRoot={attachmentRoot}
          onAttachmentRootChange={updateAttachmentRoot}
          appleContacts={appleContacts}
          onAppleContactsChange={updateAppleContacts}
          pathStats={pathStats}
          whatsappKey={whatsappKey}
          onWhatsappKeyChange={setWhatsappKey}
          showWhatsappKey={showWhatsappKey}
          onToggleWhatsappKey={() => setShowWhatsappKey((v) => !v)}
          whatsappWa={whatsappWa}
          onWhatsappWaChange={updateWhatsappWa}
          whatsappMedia={whatsappMedia}
          onWhatsappMediaChange={updateWhatsappMedia}
          whatsappDb={whatsappDb}
          onWhatsappDbChange={updateWhatsappDb}
          whatsappBusiness={whatsappBusiness}
          onWhatsappBusinessChange={setWhatsappBusiness}
          whatsappStats={whatsappStats}
          attachmentMedia={attachmentMedia}
          onAttachmentMediaChange={setAttachmentMedia}
          maxResolution={maxResolution}
          onMaxResolutionChange={setMaxResolution}
          maxFps={maxFps}
          onMaxFpsChange={setMaxFps}
          minSizeMb={minSizeMb}
          onMinSizeMbChange={setMinSizeMb}
          ownerPhones={ownerPhones}
          onOwnerPhonesChange={(phones) => {
            ownerPhonesSeededRef.current = true;
            setOwnerPhones(phones);
          }}
          ownerEmails={ownerEmails}
          onOwnerEmailsChange={(value) => {
            ownerEmailsSeededRef.current = true;
            setOwnerEmails(value);
          }}
          profilePhones={profilePhones}
          profilePhonesReady={profilePhonesReady}
          profilePhonesError={profilePhonesError}
          showMissingAccountPhoneWarning={
            profilePhonesReady && !profilePhonesError && profilePhones.length === 0
          }
          formatOpen={formatOpen}
          onToggleFormat={() => setFormatOpen((o) => !o)}
          processingOpen={processingOpen}
          onToggleProcessing={() => setProcessingOpen((o) => !o)}
          force={force}
          onForceChange={setForce}
          obfuscate={obfuscate}
          onObfuscateChange={setObfuscate}
          running={running}
          onImport={(flushedPhones) =>
            void startImport({
              source,
              backupPath,
              backupPassword,
              attachmentMedia,
              maxResolution,
              maxFps,
              minSizeMb,
              ownerPhones: flushedPhones ?? ownerPhones,
              ownerEmails: splitEmails(ownerEmails),
              force,
              obfuscate,
              isAndroidSms,
              attachmentRoot,
              appleContacts,
              whatsappKey,
              whatsappWa,
              whatsappMedia,
              whatsappDb,
              whatsappBusiness,
            })
          }
        />
      )}

      {phase === "form" && resumeChecked && resume.kind !== "none" && (
        <ResumeImportPanel
          decision={resume}
          error={resumeError}
          onResume={() => void handleResumeAction()}
          onDiscard={() => void handleDiscardResume()}
        />
      )}

      {(phase === "progress" || phase === "done") && (
        <ImportProgressView
          phase={phase}
          steps={steps}
          running={running}
          summaryView={summaryView}
          stagingDir={stagingDir}
          completionText={completionText}
          onCancel={() => void cancel()}
          onBack={returnToForm}
          cancelDisabled={computingSummary}
        />
      )}

      {phase === "identity_stop" && sourceIdentities && (
        <BackupIdentityStopScreen
          identities={sourceIdentities}
          profile={identityProfile}
          onAdd={addIdentityToProfile}
          onContinue={() => void continueAfterIdentityStop()}
          onCancel={cancelIdentityStop}
          busy={running || identityAddBusy}
          error={identityAddError}
        />
      )}

      {phase === "gate_1" && gateSummary && (
        <GateOneScreen
          summary={gateSummary}
          unknownContacts={unknownContacts}
          mode={gateAttachmentMedia}
          onApprove={() => void approveGate()}
          onDecline={() => void declineGate()}
          busy={running}
          mediaToolsMissing={mediaToolsMissing}
          mediaPartiallyRan={mediaPartiallyRan}
          identityPanel={
            sourceIdentities != null ? (
              <BackupIdentityList
                identities={sourceIdentities}
                profile={identityProfile}
                onAdd={addIdentityToProfile}
                busy={running || identityAddBusy}
                error={identityAddError}
              />
            ) : undefined
          }
        />
      )}

      {phase === "gate_2" && gateSummary && gateDelta && (
        <GateTwoScreen
          delta={gateDelta}
          actual={gateSummary}
          mode={gateAttachmentMedia}
          onApprove={() => void approveGate()}
          onDecline={() => void declineGate()}
          busy={running}
        />
      )}
    </div>
  );
}

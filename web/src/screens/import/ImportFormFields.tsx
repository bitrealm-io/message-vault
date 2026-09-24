import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import Button from "../../components/Button";
import Checkbox from "../../components/Checkbox";
import PasswordField from "../../components/PasswordField";
import PathPicker from "../../components/PathPicker";
import PhoneTokenField, { type PhoneTokenFieldHandle } from "../../components/PhoneTokenField";
import Select, { ListBoxItem, selectItemClassName } from "../../components/Select";
import TextField from "../../components/TextField";
import TimeZoneField from "../../components/TimeZoneField";
import {
  backupFolderHint,
  isAndroidSmsSource,
  needsOwnerEmails,
  splitEmails,
} from "../../lib/androidSmsSources";
import { EXPORT_SOURCES, IMAZING_SOURCE_ID } from "../../lib/exportSources";
import {
  IMESSAGE_SOURCE_ID,
  type ImessagePathStats,
  imessageAttachmentRootRequired,
  imessageCanImport,
  imessageShowsAppleContacts,
  imessageShowsAttachmentRoot,
  imessageShowsPassword,
  imessageVisiblePlatforms,
  isImessageMethod,
} from "../../lib/imessageImport";
import { ownerPhonesNeedMismatchAck } from "../../lib/phoneTokens";
import { parseSelectKey } from "../../lib/selectKey";
import type { AttachmentMediaMode } from "../../lib/types";
import { accentLink } from "../../lib/uiStyles";
import {
  isWhatsappMethod,
  WHATSAPP_METHODS,
  WHATSAPP_SOURCE_ID,
  type WhatsappPathStats,
  whatsappCanImport,
  whatsappCryptRequired,
  whatsappOwnerPhoneRequired,
  whatsappShowsBusiness,
  whatsappShowsContactsDb,
  whatsappShowsDb,
  whatsappShowsKey,
  whatsappShowsMedia,
} from "../../lib/whatsappImport";
import {
  ATTACHMENT_OPTIONS,
  CollapsibleSection,
  fieldStyle,
  hintStyle,
  RESOLUTION_OPTIONS,
  StackedField,
  sectionGap,
} from "./ImportFormUi";

export type ImportFormFieldsProps = {
  source: string;
  onSourceChange: (source: string) => void;
  backupPath: string;
  onBackupPathChange: (path: string) => void;
  backupPassword: string;
  onBackupPasswordChange: (value: string) => void;
  showBackupPassword: boolean;
  onToggleBackupPassword: () => void;
  attachmentRoot: string;
  onAttachmentRootChange: (path: string) => void;
  appleContacts: string;
  onAppleContactsChange: (path: string) => void;
  pathStats: ImessagePathStats;
  whatsappKey: string;
  onWhatsappKeyChange: (value: string) => void;
  showWhatsappKey: boolean;
  onToggleWhatsappKey: () => void;
  whatsappWa: string;
  onWhatsappWaChange: (path: string) => void;
  whatsappMedia: string;
  onWhatsappMediaChange: (path: string) => void;
  whatsappDb: string;
  onWhatsappDbChange: (path: string) => void;
  whatsappBusiness: boolean;
  onWhatsappBusinessChange: (value: boolean) => void;
  /** The holder's WhatsApp number: required on Android, a fallback on iPhone. */
  whatsappOwnerPhone: string;
  onWhatsappOwnerPhoneChange: (value: string) => void;
  whatsappStats: WhatsappPathStats;
  attachmentMedia: AttachmentMediaMode;
  onAttachmentMediaChange: (mode: AttachmentMediaMode) => void;
  maxResolution: string;
  onMaxResolutionChange: (value: string) => void;
  maxFps: string;
  onMaxFpsChange: (value: string) => void;
  minSizeMb: string;
  onMinSizeMbChange: (value: string) => void;
  ownerPhones: string[];
  onOwnerPhonesChange: (phones: string[]) => void;
  /** Owner email addresses as typed (SMS Backup+ only); commas separate several. */
  ownerEmails: string;
  onOwnerEmailsChange: (value: string) => void;
  /** Vault account phones for SBR mismatch checks (empty until loaded). */
  profilePhones: string[];
  profilePhonesReady: boolean;
  /** True when the profile request failed (fail open on mismatch gate). */
  profilePhonesError: boolean;
  showMissingAccountPhoneWarning: boolean;
  formatOpen: boolean;
  onToggleFormat: () => void;
  processingOpen: boolean;
  onToggleProcessing: () => void;
  force: boolean;
  onForceChange: (value: boolean) => void;
  obfuscate: boolean;
  onObfuscateChange: (value: boolean) => void;
  /** The IANA zone iMazing dates are read in; shown only for that source. */
  timeZone: string;
  onTimeZoneChange: (zone: string) => void;
  running: boolean;
  /** Optional flushed owner phones (SBR commits draft before import). */
  onImport: (ownerPhones?: string[]) => void;
};

const SQLITE_DB_FILTERS = [{ name: "SQLite database", extensions: ["db"] }];
const WHATSAPP_CONTACTS_FILTERS = [{ name: "SQLite database", extensions: ["db", "sqlite"] }];
const APPLE_CONTACTS_FILTERS = [{ name: "Apple AddressBook", extensions: ["abcddb", "sqlitedb"] }];

const WHATSAPP_FOLDER_HINT_ANDROID =
  "Folder that contains msgstore.db or msgstore.db.crypt12 / crypt14 / crypt15.";
const WHATSAPP_FOLDER_HINT_IPHONE = "Path to the root of a device backup";
const WHATSAPP_KEY_HINT =
  "Key file or crypt15 hex. Needed when the folder has an encrypted backup and no msgstore.db.";
const WHATSAPP_CONTACTS_HINT_ANDROID = "Leave empty if wa.db is in the backup folder.";
const WHATSAPP_CONTACTS_HINT_IPHONE = "Leave empty if ContactsV2.sqlite is in the backup.";
const WHATSAPP_MEDIA_HINT = "Leave empty if the WhatsApp media folder is in the backup folder.";
const WHATSAPP_DB_HINT = "Leave empty if msgstore.db is in the backup folder.";
const WHATSAPP_OWNER_PHONE_LABEL = "WhatsApp phone number";
const WHATSAPP_OWNER_PHONE_HINT_ANDROID =
  "Pre-filled from your profile. The number your WhatsApp account is registered to.";
const WHATSAPP_OWNER_PHONE_HINT_IPHONE =
  "Fallback, used when the backup does not contain your phone number.";

const ATTACHMENT_FOLDER_HINT_MAC =
  "Leave empty if Attachments and StickerCache are next to chat.db. Set this only when those folders live somewhere else.";
const ATTACHMENT_FOLDER_HINT_JAILBREAK = "Folder that contains Attachments and StickerCache.";
const APPLE_CONTACTS_HINT_MAC =
  "Default: use the local AddressBook. Pick AddressBook-v22.abcddb or AddressBook.sqlitedb only if that file is not in the usual Contacts location.";
const APPLE_CONTACTS_HINT_JAILBREAK =
  "AddressBook-v22.abcddb or AddressBook.sqlitedb. A local Mac AddressBook scan will not find a phone copy.";

const attachmentHelp: Record<AttachmentMediaMode, string> = {
  copy: "Copy all files as is",
  convert: "Convert all files to common formats (.jpg, .mp4, .mp3) at high quality",
  compress: "Re-encodes for smaller file size at the expense of some quality",
  skip: "Do not copy files",
};

function FieldStatus({ message }: { message: string | undefined }) {
  if (!message) return null;
  return (
    <p className={hintStyle} role="status">
      {message}
    </p>
  );
}

function AttachmentFields(props: {
  attachmentMedia: AttachmentMediaMode;
  onAttachmentMediaChange: (mode: AttachmentMediaMode) => void;
  showCompress: boolean;
  maxResolution: string;
  onMaxResolutionChange: (value: string) => void;
  maxFps: string;
  onMaxFpsChange: (value: string) => void;
  minSizeMb: string;
  onMinSizeMbChange: (value: string) => void;
}) {
  return (
    <>
      <StackedField label="Attachments">
        <Select
          selectedKey={props.attachmentMedia}
          onSelectionChange={(k) => {
            const mode = parseSelectKey(k, ["copy", "convert", "compress", "skip"] as const);
            if (mode) props.onAttachmentMediaChange(mode);
          }}
          aria-label="Attachments"
          triggerClassName="!bg-bg"
        >
          {ATTACHMENT_OPTIONS.map((o) => (
            <ListBoxItem key={o.id} id={o.id} className={selectItemClassName}>
              {o.label}
            </ListBoxItem>
          ))}
        </Select>
        <p className={hintStyle}>{attachmentHelp[props.attachmentMedia]}</p>
      </StackedField>

      {props.showCompress && (
        <div className="mb-[1.1rem] ml-4">
          <StackedField label="Target resolution">
            <Select
              selectedKey={props.maxResolution}
              onSelectionChange={(k) => props.onMaxResolutionChange(String(k))}
              aria-label="Target resolution"
              triggerClassName="!bg-bg"
            >
              {RESOLUTION_OPTIONS.map((r) => (
                <ListBoxItem key={r} id={r} className={selectItemClassName}>
                  {r.replace("p", "")}
                </ListBoxItem>
              ))}
            </Select>
            <p className={hintStyle}>Maximum video resolution; videos are not upscaled.</p>
          </StackedField>
          <StackedField label="Max FPS">
            <input
              type="text"
              value={props.maxFps}
              onChange={(e) => props.onMaxFpsChange(e.target.value)}
              className={fieldStyle}
            />
            <p className={hintStyle}>
              Maximum video frame rate; videos are not upscaled to this FPS.
            </p>
          </StackedField>
          <StackedField label="Minimum Video File Size (Megabytes)">
            <input
              type="text"
              value={props.minSizeMb}
              onChange={(e) => props.onMinSizeMbChange(e.target.value)}
              className={fieldStyle}
            />
            <p className={hintStyle}>Only re-encode videos above this size.</p>
          </StackedField>
        </div>
      )}
    </>
  );
}

export default function ImportFormFields(props: ImportFormFieldsProps) {
  const isIos = props.source === "imessage-ios";
  const isImazing = props.source === IMAZING_SOURCE_ID;
  const isAndroidSms = isAndroidSmsSource(props.source);
  const wantsEmails = needsOwnerEmails(props.source);
  const hasOwnerEmail = !wantsEmails || splitEmails(props.ownerEmails).length > 0;
  const imessageMethod = isImessageMethod(props.source) ? props.source : null;
  const whatsappMethod = isWhatsappMethod(props.source) ? props.source : null;
  const imessageGate = imessageMethod
    ? imessageCanImport({
        method: imessageMethod,
        backupPath: props.backupPath,
        attachmentRoot: props.attachmentRoot,
        appleContacts: props.appleContacts,
        backupPassword: props.backupPassword,
        stats: props.pathStats,
      })
    : null;
  const whatsappGate = whatsappMethod
    ? whatsappCanImport({
        method: whatsappMethod,
        backupPath: props.backupPath,
        key: props.whatsappKey,
        contactsDb: props.whatsappWa,
        media: props.whatsappMedia,
        db: props.whatsappDb,
        ownerPhone: props.whatsappOwnerPhone,
        stats: props.whatsappStats,
      })
    : null;
  const imessageErrors = imessageGate?.errors ?? {};
  const whatsappErrors = whatsappGate?.errors ?? {};
  const whatsappKeyRequired = whatsappMethod
    ? whatsappCryptRequired(props.whatsappStats.hasMsgstoreDb, props.whatsappStats.cryptName)
    : false;
  const showCompress =
    (imessageMethod !== null || whatsappMethod !== null || isAndroidSms) &&
    props.attachmentMedia === "compress";
  const phoneFieldRef = useRef<PhoneTokenFieldHandle>(null);
  const [phoneDraft, setPhoneDraft] = useState("");
  const [mismatchAck, setMismatchAck] = useState(false);
  const phoneDraftPending = phoneDraft.trim().length > 0;
  const phonesForMatch = phoneDraftPending
    ? [...props.ownerPhones, phoneDraft.trim()]
    : props.ownerPhones;
  const phonesMismatch =
    isAndroidSms &&
    ownerPhonesNeedMismatchAck(phonesForMatch, props.profilePhones, {
      ready: props.profilePhonesReady,
      fetchFailed: props.profilePhonesError,
    });

  useEffect(() => {
    if (!phonesMismatch) setMismatchAck(false);
  }, [phonesMismatch]);

  useEffect(() => {
    if (!isAndroidSms) {
      setPhoneDraft("");
      setMismatchAck(false);
    }
  }, [isAndroidSms]);

  const canImport = imessageGate
    ? imessageGate.enabled && !props.running
    : whatsappGate
      ? whatsappGate.enabled && !props.running
      : Boolean(props.backupPath) &&
        !props.running &&
        (!isAndroidSms || props.profilePhonesReady) &&
        (!isAndroidSms || props.ownerPhones.length > 0 || phoneDraftPending) &&
        hasOwnerEmail &&
        (!phonesMismatch || mismatchAck);

  function handleImport(): void {
    if (isAndroidSms) {
      if (!props.profilePhonesReady) return;
      const phones = phoneFieldRef.current?.flush() ?? props.ownerPhones;
      if (phones.length === 0) return;
      if (!hasOwnerEmail) return;
      const mismatch = ownerPhonesNeedMismatchAck(phones, props.profilePhones, {
        ready: props.profilePhonesReady,
        fetchFailed: props.profilePhonesError,
      });
      if (mismatch && !mismatchAck) return;
      props.onImport(phones);
      return;
    }
    props.onImport();
  }

  return (
    <>
      <h1 className="m-0 mb-1 text-2xl font-bold">Import Messages</h1>
      <p className="m-0 mb-5 text-[0.875rem] text-muted">Select your messages.</p>

      <CollapsibleSection
        title="Import Messages"
        open={props.formatOpen}
        onToggle={props.onToggleFormat}
      >
        <div className={sectionGap}>
          <Select
            selectedKey={
              imessageMethod
                ? IMESSAGE_SOURCE_ID
                : whatsappMethod
                  ? WHATSAPP_SOURCE_ID
                  : props.source
            }
            onSelectionChange={(k) => {
              const key = String(k);
              props.onSourceChange(key);
            }}
            aria-label="Import source"
            triggerClassName="!bg-bg"
          >
            {EXPORT_SOURCES.map((s) => (
              <ListBoxItem key={s.id} id={s.id} className={selectItemClassName}>
                {s.label}
              </ListBoxItem>
            ))}
          </Select>
        </div>

        {imessageMethod ? (
          <StackedField label="Platform">
            <Select
              selectedKey={props.source}
              onSelectionChange={(k) => {
                const key = String(k);
                if (isImessageMethod(key)) props.onSourceChange(key);
              }}
              aria-label="Platform"
              triggerClassName="!bg-bg"
            >
              {imessageVisiblePlatforms(imessageMethod).map((m) => (
                <ListBoxItem key={m.id} id={m.id} className={selectItemClassName}>
                  {m.label}
                </ListBoxItem>
              ))}
            </Select>
          </StackedField>
        ) : whatsappMethod ? (
          <StackedField label="Platform">
            <Select
              selectedKey={props.source}
              onSelectionChange={(k) => {
                const key = String(k);
                if (isWhatsappMethod(key)) props.onSourceChange(key);
              }}
              aria-label="Platform"
              triggerClassName="!bg-bg"
            >
              {WHATSAPP_METHODS.map((m) => (
                <ListBoxItem key={m.id} id={m.id} className={selectItemClassName}>
                  {m.label}
                </ListBoxItem>
              ))}
            </Select>
          </StackedField>
        ) : null}

        {imessageMethod ? (
          <>
            {imessageMethod === "imessage-ios" ? (
              <StackedField label="iPhone Backup Directory" required>
                <PathPicker
                  value={props.backupPath}
                  onChange={props.onBackupPathChange}
                  directory
                  placeholder="Path to the root of a device backup"
                />
                <FieldStatus message={imessageErrors.backupPath} />
              </StackedField>
            ) : (
              <StackedField label="Messages database" required>
                <PathPicker
                  value={props.backupPath}
                  onChange={props.onBackupPathChange}
                  placeholder={
                    imessageMethod === "imessage-macos" ? "Path to chat.db" : "Path to sms.db"
                  }
                  filters={SQLITE_DB_FILTERS}
                />
                <FieldStatus message={imessageErrors.backupPath} />
              </StackedField>
            )}

            {imessageShowsPassword(imessageMethod) ? (
              <StackedField
                label="Encryption password"
                required={props.pathStats.backupEncrypted === true}
                optional={props.pathStats.backupEncrypted !== true}
              >
                <PasswordField
                  aria-label={
                    props.pathStats.backupEncrypted === true
                      ? "Encryption password"
                      : "Encryption password (Optional)"
                  }
                  value={props.backupPassword}
                  onChange={props.onBackupPasswordChange}
                  autoComplete="new-password"
                  showPassword={props.showBackupPassword}
                  onToggle={props.onToggleBackupPassword}
                />
                <FieldStatus message={imessageErrors.backupPassword} />
              </StackedField>
            ) : null}

            {imessageShowsAttachmentRoot(imessageMethod) ? (
              <StackedField
                label="Attachment folder"
                required={imessageAttachmentRootRequired(imessageMethod)}
                optional={!imessageAttachmentRootRequired(imessageMethod)}
              >
                <PathPicker
                  value={props.attachmentRoot}
                  onChange={props.onAttachmentRootChange}
                  directory
                />
                <p className={hintStyle}>
                  {imessageMethod === "imessage-macos"
                    ? ATTACHMENT_FOLDER_HINT_MAC
                    : ATTACHMENT_FOLDER_HINT_JAILBREAK}
                </p>
                <FieldStatus message={imessageErrors.attachmentRoot} />
              </StackedField>
            ) : null}

            {imessageShowsAppleContacts(imessageMethod) ? (
              <StackedField label="Apple Contacts file" optional>
                <PathPicker
                  value={props.appleContacts}
                  onChange={props.onAppleContactsChange}
                  filters={APPLE_CONTACTS_FILTERS}
                />
                <p className={hintStyle}>
                  {imessageMethod === "imessage-macos"
                    ? APPLE_CONTACTS_HINT_MAC
                    : APPLE_CONTACTS_HINT_JAILBREAK}
                </p>
                <FieldStatus message={imessageErrors.appleContacts} />
              </StackedField>
            ) : null}

            <AttachmentFields
              attachmentMedia={props.attachmentMedia}
              onAttachmentMediaChange={props.onAttachmentMediaChange}
              showCompress={showCompress}
              maxResolution={props.maxResolution}
              onMaxResolutionChange={props.onMaxResolutionChange}
              maxFps={props.maxFps}
              onMaxFpsChange={props.onMaxFpsChange}
              minSizeMb={props.minSizeMb}
              onMinSizeMbChange={props.onMinSizeMbChange}
            />
          </>
        ) : whatsappMethod ? (
          <>
            <StackedField label="Backup folder" required>
              <PathPicker value={props.backupPath} onChange={props.onBackupPathChange} directory />
              <p className={hintStyle}>
                {whatsappMethod === "whatsapp-ios"
                  ? WHATSAPP_FOLDER_HINT_IPHONE
                  : WHATSAPP_FOLDER_HINT_ANDROID}
              </p>
              <FieldStatus message={whatsappErrors.backupPath} />
            </StackedField>

            {whatsappShowsKey(whatsappMethod) ? (
              <StackedField
                label="Decryption key"
                required={whatsappKeyRequired}
                optional={!whatsappKeyRequired}
              >
                <PasswordField
                  aria-label={whatsappKeyRequired ? "Decryption key" : "Decryption key (Optional)"}
                  value={props.whatsappKey}
                  onChange={props.onWhatsappKeyChange}
                  autoComplete="new-password"
                  showPassword={props.showWhatsappKey}
                  onToggle={props.onToggleWhatsappKey}
                />
                <p className={hintStyle}>{WHATSAPP_KEY_HINT}</p>
                <FieldStatus message={whatsappErrors.key} />
              </StackedField>
            ) : null}

            {whatsappOwnerPhoneRequired(whatsappMethod) ? (
              <StackedField label={WHATSAPP_OWNER_PHONE_LABEL} required>
                <input
                  type="text"
                  inputMode="tel"
                  aria-label={WHATSAPP_OWNER_PHONE_LABEL}
                  value={props.whatsappOwnerPhone}
                  onChange={(e) => props.onWhatsappOwnerPhoneChange(e.target.value)}
                  placeholder="+1 555 555 0100"
                  className={fieldStyle}
                />
                <p className={hintStyle}>{WHATSAPP_OWNER_PHONE_HINT_ANDROID}</p>
                <FieldStatus message={whatsappErrors.ownerPhone} />
              </StackedField>
            ) : null}

            {whatsappShowsContactsDb(whatsappMethod) ? (
              <StackedField label="Contacts database" optional>
                <PathPicker
                  value={props.whatsappWa}
                  onChange={props.onWhatsappWaChange}
                  filters={WHATSAPP_CONTACTS_FILTERS}
                />
                <p className={hintStyle}>
                  {whatsappMethod === "whatsapp-ios"
                    ? WHATSAPP_CONTACTS_HINT_IPHONE
                    : WHATSAPP_CONTACTS_HINT_ANDROID}
                </p>
                <FieldStatus message={whatsappErrors.contactsDb} />
              </StackedField>
            ) : null}

            {whatsappShowsMedia(whatsappMethod) ? (
              <StackedField label="Media folder" optional>
                <PathPicker
                  value={props.whatsappMedia}
                  onChange={props.onWhatsappMediaChange}
                  directory
                />
                <p className={hintStyle}>{WHATSAPP_MEDIA_HINT}</p>
                <FieldStatus message={whatsappErrors.media} />
              </StackedField>
            ) : null}

            {whatsappShowsDb(whatsappMethod) ? (
              <StackedField label="Message database" optional>
                <PathPicker
                  value={props.whatsappDb}
                  onChange={props.onWhatsappDbChange}
                  filters={SQLITE_DB_FILTERS}
                />
                <p className={hintStyle}>{WHATSAPP_DB_HINT}</p>
                <FieldStatus message={whatsappErrors.db} />
              </StackedField>
            ) : null}

            {whatsappShowsBusiness(whatsappMethod) ? (
              <Checkbox
                labelClassName="mb-[1.1rem] flex text-[0.875rem]"
                checked={props.whatsappBusiness}
                onChange={props.onWhatsappBusinessChange}
              >
                WhatsApp Business
              </Checkbox>
            ) : null}

            <AttachmentFields
              attachmentMedia={props.attachmentMedia}
              onAttachmentMediaChange={props.onAttachmentMediaChange}
              showCompress={showCompress}
              maxResolution={props.maxResolution}
              onMaxResolutionChange={props.onMaxResolutionChange}
              maxFps={props.maxFps}
              onMaxFpsChange={props.onMaxFpsChange}
              minSizeMb={props.minSizeMb}
              onMinSizeMbChange={props.onMinSizeMbChange}
            />
          </>
        ) : isAndroidSms ? (
          <>
            <StackedField label="Backup Directory">
              <PathPicker
                value={props.backupPath}
                onChange={props.onBackupPathChange}
                directory
                placeholder="Folder containing sms-*.xml backup files"
              />
              <p className={hintStyle}>{backupFolderHint(props.source)}</p>
            </StackedField>

            <AttachmentFields
              attachmentMedia={props.attachmentMedia}
              onAttachmentMediaChange={props.onAttachmentMediaChange}
              showCompress={showCompress}
              maxResolution={props.maxResolution}
              onMaxResolutionChange={props.onMaxResolutionChange}
              maxFps={props.maxFps}
              onMaxFpsChange={props.onMaxFpsChange}
              minSizeMb={props.minSizeMb}
              onMinSizeMbChange={props.onMinSizeMbChange}
            />

            <StackedField label="Backup Device Phone Numbers">
              <PhoneTokenField
                ref={phoneFieldRef}
                value={props.ownerPhones}
                onChange={props.onOwnerPhonesChange}
                onDraftChange={setPhoneDraft}
                aria-label="Backup Device Phone Numbers"
              />
              <p className={hintStyle}>
                Pre-filled from your profile. Add numbers from other SIMs, if needed.
              </p>
              {props.showMissingAccountPhoneWarning ? (
                <div
                  role="status"
                  className="mt-2 rounded-lg border border-warn-soft-border bg-warn-soft-bg px-3 py-2 text-[0.8125rem] text-warn-soft-text"
                >
                  Your user profile is missing a phone number. Add one in{" "}
                  <Link to="/settings?tab=profile" className={`${accentLink} text-[0.8125rem]`}>
                    Settings → Profile
                  </Link>{" "}
                  so import can tell which messages you sent.
                </div>
              ) : null}
              {phonesMismatch && !mismatchAck && phonesForMatch.length > 0 ? (
                <div
                  role="status"
                  className="mt-2 rounded-lg border border-warn-soft-border bg-warn-soft-bg px-3 py-2 text-[0.8125rem] text-warn-soft-text"
                >
                  I understand none of the entered phone numbers match my profile and that imported
                  messages will not be linked to my account.
                </div>
              ) : null}
              <Checkbox
                labelClassName="mt-2 flex items-start text-[0.8125rem]"
                className="mt-0.5 shrink-0"
                checked={mismatchAck}
                onChange={setMismatchAck}
              >
                <span>Allow import from phone numbers not on my profile.</span>
              </Checkbox>
            </StackedField>

            {wantsEmails ? (
              <TextField
                label="Backup Device Email Addresses"
                aria-label="Backup Device Email Addresses"
                value={props.ownerEmails}
                onChange={props.onOwnerEmailsChange}
                hint="Pre-filled from your profile. The Gmail or IMAP account SMS Backup+ synced to; separate several with commas."
                placeholder="you@example.com"
              />
            ) : null}
          </>
        ) : (
          <StackedField label="Backup path">
            <PathPicker value={props.backupPath} onChange={props.onBackupPathChange} directory />
          </StackedField>
        )}
      </CollapsibleSection>

      <CollapsibleSection
        title="Processing Options (Advanced)"
        open={props.processingOpen}
        onToggle={props.onToggleProcessing}
      >
        <div className="mb-2 flex flex-col items-start gap-3">
          <Checkbox
            labelClassName="text-[0.875rem]"
            checked={props.force}
            onChange={props.onForceChange}
          >
            Force reprocessing
          </Checkbox>
          {isIos || isAndroidSms ? (
            <Checkbox
              labelClassName="text-[0.875rem]"
              checked={props.obfuscate}
              onChange={props.onObfuscateChange}
            >
              Obfuscate - All message data is anonymized.
            </Checkbox>
          ) : null}
          {isImazing ? (
            <div className="w-full max-w-[28rem]">
              <TimeZoneField
                label="Time zone of the messages"
                value={props.timeZone}
                onChange={props.onTimeZoneChange}
              />
              <p className={hintStyle}>
                Pre-filled from your profile. iMazing writes each message time without a zone, so
                pick the one the phone was in.
              </p>
            </div>
          ) : null}
        </div>
        {whatsappMethod !== null && !whatsappOwnerPhoneRequired(whatsappMethod) ? (
          <StackedField label={WHATSAPP_OWNER_PHONE_LABEL} optional>
            <input
              type="text"
              inputMode="tel"
              aria-label={`${WHATSAPP_OWNER_PHONE_LABEL} (Optional)`}
              value={props.whatsappOwnerPhone}
              onChange={(e) => props.onWhatsappOwnerPhoneChange(e.target.value)}
              placeholder="+1 555 555 0100"
              className={fieldStyle}
            />
            <p className={hintStyle}>{WHATSAPP_OWNER_PHONE_HINT_IPHONE}</p>
          </StackedField>
        ) : null}
      </CollapsibleSection>

      <div className="mt-2 flex gap-3">
        <Button
          variant="primary"
          onClick={handleImport}
          disabled={!canImport}
          className="!rounded-lg !px-6 !py-[0.55rem]"
        >
          Import
        </Button>
      </div>
    </>
  );
}

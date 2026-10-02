/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { holdDesktopJob } from "../../lib/desktopJob";
import { EXPORT_SOURCES } from "../../lib/exportSources";
import { IMESSAGE_METHODS, IMESSAGE_SOURCE_ID } from "../../lib/imessageImport";
import {
  emptyWhatsappPathStats,
  WHATSAPP_ERR_CRYPT_KEY,
  WHATSAPP_ERR_FOLDER_IS_FILE,
  WHATSAPP_ERR_OWNER_PHONE,
  WHATSAPP_METHODS,
  WHATSAPP_SOURCE_ID,
} from "../../lib/whatsappImport";
import ImportFormFields, { type ImportFormFieldsProps } from "./ImportFormFields";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

afterEach(() => {
  cleanup();
});

const presentFile = { exists: true, isFile: true, isDirectory: false };
const presentDir = { exists: true, isFile: false, isDirectory: true };

function renderForm(override: Partial<ImportFormFieldsProps> = {}) {
  const props: ImportFormFieldsProps = {
    source: "imessage-ios",
    onSourceChange: vi.fn(),
    backupPath: "/backups/iphone",
    onBackupPathChange: vi.fn(),
    backupPassword: "",
    onBackupPasswordChange: vi.fn(),
    showBackupPassword: false,
    onToggleBackupPassword: vi.fn(),
    attachmentRoot: "",
    onAttachmentRootChange: vi.fn(),
    appleContacts: "",
    onAppleContactsChange: vi.fn(),
    pathStats: {
      backup: presentDir,
      attachmentRoot: null,
      appleContacts: null,
      backupEncrypted: false,
    },
    whatsappKey: "",
    onWhatsappKeyChange: vi.fn(),
    showWhatsappKey: false,
    onToggleWhatsappKey: vi.fn(),
    whatsappWa: "",
    onWhatsappWaChange: vi.fn(),
    whatsappMedia: "",
    onWhatsappMediaChange: vi.fn(),
    whatsappDb: "",
    onWhatsappDbChange: vi.fn(),
    whatsappBusiness: false,
    whatsappOwnerPhone: "",
    onWhatsappOwnerPhoneChange: vi.fn(),
    onWhatsappBusinessChange: vi.fn(),
    whatsappStats: emptyWhatsappPathStats(),
    attachmentMedia: "copy",
    onAttachmentMediaChange: vi.fn(),
    maxResolution: "720p",
    onMaxResolutionChange: vi.fn(),
    maxFps: "30",
    onMaxFpsChange: vi.fn(),
    minSizeMb: "20",
    onMinSizeMbChange: vi.fn(),
    ownerPhones: [],
    onOwnerPhonesChange: vi.fn(),
    ownerEmails: "",
    onOwnerEmailsChange: vi.fn(),
    profilePhones: [],
    profilePhonesReady: true,
    profilePhonesError: false,
    showMissingAccountPhoneWarning: false,
    formatOpen: true,
    onToggleFormat: vi.fn(),
    processingOpen: false,
    onToggleProcessing: vi.fn(),
    obfuscate: false,
    onObfuscateChange: vi.fn(),
    timeZone: "America/New_York",
    onTimeZoneChange: vi.fn(),
    running: false,
    onImport: vi.fn(),
    ...override,
  };
  return render(<ImportFormFields {...props} />);
}

describe("ImportFormFields iMessage methods", () => {
  it("shows one iMessage source and a Platform dropdown without jailbreak", async () => {
    const user = userEvent.setup();
    renderForm();
    expect(screen.getByLabelText("Import source")).toBeTruthy();
    expect(screen.getByLabelText("Platform")).toBeTruthy();
    expect(screen.queryByText("iPhone - iOS")).toBeNull();
    expect(screen.queryByText("iMessage - macOS")).toBeNull();
    await user.click(screen.getByLabelText("Platform"));
    expect(await screen.findByRole("option", { name: "iPhone backup" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "Mac Messages" })).toBeTruthy();
    expect(screen.queryByRole("option", { name: "Jailbroken iPhone" })).toBeNull();
  });

  it("keeps jailbreak in Platform when that method is already selected", async () => {
    const user = userEvent.setup();
    renderForm({
      source: "imessage-jailbreak",
      backupPath: "/mnt/iphone/sms.db",
      attachmentRoot: "/mnt/iphone/Library/SMS",
      pathStats: {
        backup: presentFile,
        attachmentRoot: presentDir,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    await user.click(screen.getByLabelText("Platform"));
    expect(await screen.findByRole("option", { name: "Jailbroken iPhone" })).toBeTruthy();
  });

  it("marks the iPhone backup folder required and encryption password optional", () => {
    renderForm();
    const backupLabel = screen.getByText("iPhone Backup Directory").closest("label");
    expect(backupLabel?.textContent).toContain("*");
    expect(screen.getByLabelText("Encryption password (Optional)")).toBeTruthy();
  });

  it("marks encryption password required when the backup is encrypted", () => {
    renderForm({
      pathStats: {
        backup: presentDir,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: true,
      },
    });
    const passwordLabel = screen.getByText("Encryption password").closest("label");
    expect(passwordLabel?.textContent).toContain("*");
    expect(screen.queryByLabelText("Encryption password (Optional)")).toBeNull();
  });

  it("does not start an Import Run while another desktop job runs, and names that job", () => {
    const release = holdDesktopJob("Export");
    try {
      renderForm({ source: "imessage-ios" });
      expect(screen.getByRole("button", { name: "Import" })).toBeDisabled();
      expect(screen.getByRole("status").textContent).toBe(
        "An export is running. Import can start once it ends.",
      );
    } finally {
      release();
    }
  });

  it("shows password and hides attachment folder on iPhone backup", () => {
    renderForm({ source: "imessage-ios" });
    expect(screen.getByLabelText("Encryption password (Optional)")).toBeTruthy();
    expect(screen.queryByLabelText("Attachment folder")).toBeNull();
    expect(screen.queryByLabelText("Apple Contacts file")).toBeNull();
    expect(screen.getByRole("button", { name: "Import" })).not.toBeDisabled();
  });

  it("shows optional attachment folder on Mac Messages", () => {
    renderForm({
      source: "imessage-macos",
      backupPath: "/Users/sam/Library/Messages/chat.db",
      pathStats: {
        backup: presentFile,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(screen.queryByLabelText("Encryption password")).toBeNull();
    expect(screen.getByLabelText("Attachment folder (Optional)")).toBeTruthy();
    expect(screen.getByLabelText("Apple Contacts file (Optional)")).toBeTruthy();
    expect(
      screen.getByText(
        "Leave empty if Attachments and StickerCache are next to chat.db. Set this only when those folders live somewhere else.",
      ),
    ).toBeTruthy();
    expect(
      screen.getByText(
        "Default: use the local AddressBook. Pick AddressBook-v22.abcddb or AddressBook.sqlitedb only if that file is not in the usual Contacts location.",
      ),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "Import" })).not.toBeDisabled();
  });

  it("disables Import on jailbreak until the attachment folder is set", () => {
    renderForm({
      source: "imessage-jailbreak",
      backupPath: "/mnt/iphone/sms.db",
      attachmentRoot: "",
      pathStats: {
        backup: presentFile,
        attachmentRoot: null,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    const attachmentLabel = screen.getByText("Attachment folder").closest("label");
    expect(attachmentLabel?.textContent).toContain("*");
    expect(screen.getByRole("button", { name: "Import" })).toBeDisabled();
  });

  it("enables jailbreak Import when sms.db and attachment folder exist", () => {
    renderForm({
      source: "imessage-jailbreak",
      backupPath: "/mnt/iphone/sms.db",
      attachmentRoot: "/mnt/iphone/Library/SMS",
      pathStats: {
        backup: presentFile,
        attachmentRoot: presentDir,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(screen.getByRole("button", { name: "Import" })).not.toBeDisabled();
  });

  it("shows an attachment-folder kind error when the path is a file", () => {
    renderForm({
      source: "imessage-macos",
      backupPath: "/tmp/chat.db",
      attachmentRoot: "/tmp/chat.db",
      pathStats: {
        backup: presentFile,
        attachmentRoot: presentFile,
        appleContacts: null,
        backupEncrypted: null,
      },
    });
    expect(
      screen.getByText("Pick the folder that contains Attachments and StickerCache."),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "Import" })).toBeDisabled();
  });

  it("passes the iMessage source key through when iMessage is chosen again", async () => {
    const onSourceChange = vi.fn();
    const user = userEvent.setup();
    renderForm({ source: "whatsapp-android", onSourceChange });
    await user.click(screen.getByLabelText("Import source"));
    expect(await screen.findByRole("option", { name: "WhatsApp" })).toBeTruthy();
    await user.click(screen.getByRole("option", { name: "iMessage" }));
    expect(onSourceChange).toHaveBeenCalledWith(IMESSAGE_SOURCE_ID);
  });

  it("passes the WhatsApp source key through when WhatsApp is chosen", async () => {
    const onSourceChange = vi.fn();
    const user = userEvent.setup();
    renderForm({ source: "imessage-ios", onSourceChange });
    await user.click(screen.getByLabelText("Import source"));
    expect(await screen.findByRole("option", { name: "iMessage" })).toBeTruthy();
    await user.click(screen.getByRole("option", { name: "WhatsApp" }));
    expect(onSourceChange).toHaveBeenCalledWith(WHATSAPP_SOURCE_ID);
  });

  it("shows an Apple Contacts kind error when the path is a directory", () => {
    renderForm({
      source: "imessage-macos",
      backupPath: "/tmp/chat.db",
      appleContacts: "/tmp/AddressBook",
      pathStats: {
        backup: presentFile,
        attachmentRoot: null,
        appleContacts: presentDir,
        backupEncrypted: null,
      },
    });
    expect(screen.getByText("Pick AddressBook-v22.abcddb or AddressBook.sqlitedb.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Import" })).toBeDisabled();
  });
});

describe("ImportFormFields WhatsApp methods", () => {
  it("shows WhatsApp Platform Android and iPhone with key and attachments", async () => {
    const user = userEvent.setup();
    renderForm({ source: "whatsapp-android" });
    expect(screen.getByLabelText("Platform")).toBeTruthy();
    await user.click(screen.getByLabelText("Platform"));
    expect(await screen.findByRole("option", { name: "Android" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "iPhone" })).toBeTruthy();
    expect(screen.getByLabelText("Decryption key (Optional)")).toBeTruthy();
    expect(screen.queryByRole("checkbox", { name: "WhatsApp Business" })).toBeNull();
    expect(screen.getByLabelText("Attachments")).toBeTruthy();
  });

  it("hides key, media, and db on iPhone and shows Business", () => {
    renderForm({ source: "whatsapp-ios" });
    expect(screen.queryByLabelText("Decryption key (Optional)")).toBeNull();
    expect(screen.queryByLabelText("Decryption key")).toBeNull();
    expect(screen.getByRole("checkbox", { name: "WhatsApp Business" })).toBeTruthy();
    expect(screen.queryByLabelText("Media folder (Optional)")).toBeNull();
    expect(screen.queryByLabelText("Message database (Optional)")).toBeNull();
  });

  it("requires the decryption key for an encrypted backup", () => {
    renderForm({
      source: "whatsapp-android",
      backupPath: "/tmp/wa",
      whatsappKey: "",
      whatsappStats: {
        backup: presentDir,
        contactsDb: null,
        media: null,
        db: null,
        hasMsgstoreDb: false,
        cryptName: "msgstore.db.crypt15",
        backupEncrypted: null,
      },
    });
    expect(screen.getByText(WHATSAPP_ERR_CRYPT_KEY)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Import" })).toBeDisabled();
  });

  // An Android crypt backup carries no owner number, so the form asks for it
  // beside the other required fields and refuses an import without it.
  it("requires the owner's WhatsApp number on Android", () => {
    const stats = {
      backup: presentDir,
      contactsDb: null,
      media: null,
      db: null,
      hasMsgstoreDb: true,
      cryptName: null,
      backupEncrypted: null,
    };
    renderForm({
      source: "whatsapp-android",
      backupPath: "/tmp/wa",
      whatsappOwnerPhone: "",
      whatsappStats: stats,
    });
    expect(screen.getByLabelText("WhatsApp phone number")).toBeTruthy();
    expect(screen.getByText(WHATSAPP_ERR_OWNER_PHONE)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Import" })).toBeDisabled();
    cleanup();

    renderForm({
      source: "whatsapp-android",
      backupPath: "/tmp/wa",
      whatsappOwnerPhone: "+15555550100",
      whatsappStats: stats,
    });
    expect((screen.getByLabelText("WhatsApp phone number") as HTMLInputElement).value).toBe(
      "+15555550100",
    );
    expect(screen.queryByText(WHATSAPP_ERR_OWNER_PHONE)).toBeNull();
    expect(screen.getByRole("button", { name: "Import" })).toBeEnabled();
  });

  // An iPhone backup carries the number in WhatsApp's preferences, so the
  // field is a fallback under the advanced section and may stay empty.
  it("offers the number as an optional fallback under Processing Options on iPhone", () => {
    renderForm({
      source: "whatsapp-ios",
      backupPath: "/backups/iphone",
      whatsappOwnerPhone: "",
      whatsappStats: {
        backup: presentDir,
        contactsDb: null,
        media: null,
        db: null,
        hasMsgstoreDb: false,
        cryptName: null,
        backupEncrypted: null,
      },
      processingOpen: true,
    });
    expect(screen.queryByLabelText("WhatsApp phone number")).toBeNull();
    expect(screen.getByLabelText("WhatsApp phone number (Optional)")).toBeTruthy();
    expect(
      screen.getByText("Fallback, used when the backup does not contain your phone number."),
    ).toBeTruthy();
    expect(screen.queryByText(WHATSAPP_ERR_OWNER_PHONE)).toBeNull();
    expect(screen.getByRole("button", { name: "Import" })).toBeEnabled();
  });

  it("shows a folder-kind error when the backup path is a file", () => {
    renderForm({
      source: "whatsapp-android",
      backupPath: "/tmp/msgstore.db",
      whatsappStats: {
        backup: presentFile,
        contactsDb: null,
        media: null,
        db: null,
        hasMsgstoreDb: true,
        cryptName: null,
        backupEncrypted: null,
      },
    });
    expect(screen.getByText(WHATSAPP_ERR_FOLDER_IS_FILE)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Import" })).toBeDisabled();
  });
});

/**
 * The button the whole form leads to.
 *
 * `onImport` was in the props of every test in this file and was asserted in
 * none of them, over 320 lines. A form whose Import button was wired to
 * nothing rendered identically and passed: every test read the fields and the
 * gates, and none pressed the button.
 */
describe("ImportFormFields Import button", () => {
  it("calls onImport when the form is ready", async () => {
    const user = userEvent.setup();
    const onImport = vi.fn();
    renderForm({ onImport });

    await user.click(screen.getByRole("button", { name: "Import" }));

    expect(onImport).toHaveBeenCalledTimes(1);
  });

  it("is refused while a run is already going, so a second click cannot start one", async () => {
    const user = userEvent.setup();
    const onImport = vi.fn();
    renderForm({ onImport, running: true });

    const button = screen.getByRole("button", { name: "Import" });
    expect(button).toBeDisabled();
    await user.click(button);

    expect(onImport).not.toHaveBeenCalled();
  });

  it("is refused with no backup chosen, which is the one thing an import needs", async () => {
    const user = userEvent.setup();
    const onImport = vi.fn();
    renderForm({ onImport, backupPath: "" });

    await user.click(screen.getByRole("button", { name: "Import" }));

    expect(onImport).not.toHaveBeenCalled();
  });

  /**
   * An Android SMS import needs the owner's own number: without it the
   * exporter cannot tell which side of a conversation the person is, and every
   * message comes out incoming. The form carries the numbers to `onImport`
   * rather than leaving the caller to read them back out of state.
   */
  it("hands the owner's numbers to onImport for an Android SMS backup", async () => {
    const user = userEvent.setup();
    const onImport = vi.fn();
    renderForm({
      onImport,
      source: "sms-backup-restore",
      backupPath: "/backups/sms.xml",
      ownerPhones: ["+15555550100"],
      // The same number is on the account profile, so there is no mismatch to
      // acknowledge and the form is ready.
      profilePhones: ["+15555550100"],
      ownerEmails: "me@example.com",
    });

    await user.click(screen.getByRole("button", { name: "Import" }));

    expect(onImport).toHaveBeenCalledTimes(1);
    expect(onImport).toHaveBeenCalledWith(["+15555550100"]);
  });

  it("offers the time zone picker for iMazing alone, under Processing Options", () => {
    const { unmount } = renderForm({ source: "imazing", processingOpen: true });
    const picker = screen.getByRole("combobox", {
      name: "Time zone of the messages",
    }) as HTMLInputElement;
    // The row's label carries the offset in force today, so only the city is pinned.
    expect(picker.value).toContain("New York");
    unmount();
    renderForm({ source: "imessage-ios", processingOpen: true });
    expect(screen.queryByRole("combobox", { name: "Time zone of the messages" })).toBeNull();
  });

  // The section holds a field only for some sources. For the rest it would
  // open on nothing.
  it("shows Processing Options only for a source with a field in it", () => {
    const { unmount } = renderForm({ source: "imessage-ios" });
    expect(screen.getByText("Processing Options (Advanced)")).toBeInTheDocument();
    unmount();
    renderForm({ source: "whatsapp-android" });
    expect(screen.queryByText("Processing Options (Advanced)")).toBeNull();
  });

  it("names each Android SMS source's own backup files in the folder placeholder", () => {
    const placeholders = [
      ["sms-backup-restore", "Folder containing sms-*.xml backup files"],
      ["go-sms-pro", "Folder containing gosms_sys*.xml backup files"],
      ["sms-backup-plus", "Folder containing .eml files"],
    ];
    for (const [source, placeholder] of placeholders) {
      const { unmount } = renderForm({ source });
      expect(screen.getByPlaceholderText(placeholder)).toBeTruthy();
      unmount();
    }
  });

  // Import stays disabled without either one, so both carry the asterisk.
  it("marks the Android SMS backup folder and phone numbers required", () => {
    renderForm({ source: "sms-backup-restore" });
    const backupLabel = screen.getByText("Backup Directory").closest("label");
    expect(backupLabel?.textContent).toContain("*");
    const phonesLabel = screen.getByText("Backup Device Phone Numbers").closest("label");
    expect(phonesLabel?.textContent).toContain("*");
  });

  it("refuses an Android SMS backup with no owner number", async () => {
    const user = userEvent.setup();
    const onImport = vi.fn();
    renderForm({
      onImport,
      source: "sms-backup-restore",
      backupPath: "/backups/sms.xml",
      ownerPhones: [],
      profilePhones: ["+15555550100"],
      ownerEmails: "me@example.com",
    });

    await user.click(screen.getByRole("button", { name: "Import" }));

    expect(onImport).not.toHaveBeenCalled();
  });
});

/**
 * The asterisk and the Import button have to agree (#1031). Each case is one
 * source with every field filled, so Import is enabled, and names each field
 * the source shows with the props that empty it. The test empties one field
 * at a time: a field whose absence disables Import must carry the asterisk,
 * and a field that carries the asterisk must be one Import needs.
 */
type FormCase = {
  name: string;
  props: Partial<ImportFormFieldsProps>;
  fields: { label: string; empty: Partial<ImportFormFieldsProps> }[];
};

const filledWhatsappStats = {
  backup: presentDir,
  contactsDb: presentFile,
  media: presentDir,
  db: presentFile,
  hasMsgstoreDb: true,
  cryptName: null,
  backupEncrypted: null,
};
const whatsappAndroidProps: Partial<ImportFormFieldsProps> = {
  source: "whatsapp-android",
  backupPath: "/backups/whatsapp",
  whatsappKey: "/backups/key",
  whatsappOwnerPhone: "+15555550100",
  whatsappWa: "/backups/wa.db",
  whatsappMedia: "/backups/Media",
  whatsappDb: "/backups/msgstore.db",
  whatsappStats: filledWhatsappStats,
};
const whatsappAndroidFields: FormCase["fields"] = [
  { label: "Backup folder", empty: { backupPath: "" } },
  { label: "Decryption key", empty: { whatsappKey: "" } },
  { label: "WhatsApp phone number", empty: { whatsappOwnerPhone: "" } },
  { label: "Contacts database", empty: { whatsappWa: "" } },
  { label: "Media folder", empty: { whatsappMedia: "" } },
  { label: "Message database", empty: { whatsappDb: "" } },
];
const whatsappIphoneProps: Partial<ImportFormFieldsProps> = {
  source: "whatsapp-ios",
  backupPath: "/backups/iphone",
  backupPassword: "secret",
  whatsappOwnerPhone: "+15555550100",
  whatsappWa: "/backups/ContactsV2.sqlite",
  whatsappStats: filledWhatsappStats,
  processingOpen: true,
};
const whatsappIphoneFields: FormCase["fields"] = [
  { label: "Backup folder", empty: { backupPath: "" } },
  { label: "Encryption password", empty: { backupPassword: "" } },
  { label: "Contacts database", empty: { whatsappWa: "" } },
  { label: "WhatsApp phone number", empty: { whatsappOwnerPhone: "" } },
];
const iphoneBackupStats = {
  backup: presentDir,
  attachmentRoot: null,
  appleContacts: null,
  backupEncrypted: false,
};
const iphoneBackupFields: FormCase["fields"] = [
  { label: "iPhone Backup Directory", empty: { backupPath: "" } },
  { label: "Encryption password", empty: { backupPassword: "" } },
];
const messagesDbProps: Partial<ImportFormFieldsProps> = {
  backupPath: "/mnt/messages.db",
  attachmentRoot: "/mnt/attachments",
  appleContacts: "/mnt/AddressBook.sqlitedb",
  pathStats: {
    backup: presentFile,
    attachmentRoot: presentDir,
    appleContacts: presentFile,
    backupEncrypted: null,
  },
};
const messagesDbFields: FormCase["fields"] = [
  { label: "Messages database", empty: { backupPath: "" } },
  { label: "Attachment folder", empty: { attachmentRoot: "" } },
  { label: "Apple Contacts file", empty: { appleContacts: "" } },
];
const androidSmsProps: Partial<ImportFormFieldsProps> = {
  backupPath: "/backups/sms",
  ownerPhones: ["+15555550100"],
  profilePhones: ["+15555550100"],
  ownerEmails: "me@example.com",
};
const androidSmsFields: FormCase["fields"] = [
  { label: "Backup Directory", empty: { backupPath: "" } },
  { label: "Backup Device Phone Numbers", empty: { ownerPhones: [] } },
];

const FORM_CASES: FormCase[] = [
  {
    name: "iMessage, iPhone backup",
    props: { source: "imessage-ios", backupPassword: "secret", pathStats: iphoneBackupStats },
    fields: iphoneBackupFields,
  },
  {
    name: "iMessage, encrypted iPhone backup",
    props: {
      source: "imessage-ios",
      backupPassword: "secret",
      pathStats: { ...iphoneBackupStats, backupEncrypted: true },
    },
    fields: iphoneBackupFields,
  },
  {
    name: "iMessage, Mac Messages",
    props: { source: "imessage-macos", ...messagesDbProps },
    fields: messagesDbFields,
  },
  {
    name: "iMessage, jailbroken iPhone",
    props: { source: "imessage-jailbreak", ...messagesDbProps },
    fields: messagesDbFields,
  },
  { name: "WhatsApp, Android", props: whatsappAndroidProps, fields: whatsappAndroidFields },
  {
    name: "WhatsApp, encrypted Android backup",
    props: {
      ...whatsappAndroidProps,
      whatsappStats: {
        ...filledWhatsappStats,
        hasMsgstoreDb: false,
        cryptName: "msgstore.db.crypt15",
        backupEncrypted: null,
      },
    },
    fields: whatsappAndroidFields,
  },
  {
    name: "WhatsApp, iPhone",
    props: whatsappIphoneProps,
    fields: whatsappIphoneFields,
  },
  {
    name: "WhatsApp, encrypted iPhone backup",
    props: {
      ...whatsappIphoneProps,
      whatsappStats: { ...filledWhatsappStats, backupEncrypted: true },
    },
    fields: whatsappIphoneFields,
  },
  {
    name: "SMS Backup & Restore",
    props: { source: "sms-backup-restore", ...androidSmsProps },
    fields: androidSmsFields,
  },
  {
    name: "GO SMS Pro",
    props: { source: "go-sms-pro", ...androidSmsProps },
    fields: androidSmsFields,
  },
  {
    name: "SMS Backup+",
    props: { source: "sms-backup-plus", ...androidSmsProps },
    fields: [
      ...androidSmsFields,
      { label: "Backup Device Email Addresses", empty: { ownerEmails: "" } },
    ],
  },
  {
    name: "iMazing",
    props: { source: "imazing", backupPath: "/backups/imazing" },
    fields: [{ label: "Backup path", empty: { backupPath: "" } }],
  },
  {
    name: "OpenExtract",
    props: { source: "openextract", backupPath: "/backups/openextract" },
    fields: [{ label: "Backup path", empty: { backupPath: "" } }],
  },
];

/** The labels on screen that carry the asterisk, without it. */
function markedLabels(): string[] {
  return Array.from(document.querySelectorAll("label"))
    .map((label) => label.textContent ?? "")
    .filter((text) => text.endsWith(" *"))
    .map((text) => text.slice(0, -2))
    .sort();
}

function importButton(): HTMLElement {
  return screen.getByRole("button", { name: "Import" });
}

describe("ImportFormFields required marks", () => {
  // A source added to the Import source list, or a platform added under
  // iMessage or WhatsApp, fails here until its fields are listed above.
  it("has a case for every import source", () => {
    const covered = new Set(FORM_CASES.map((c) => c.props.source));
    const sources = [
      ...IMESSAGE_METHODS.map((m) => m.id),
      ...WHATSAPP_METHODS.map((m) => m.id),
      ...EXPORT_SOURCES.map((s) => s.id).filter(
        (id) => id !== IMESSAGE_SOURCE_ID && id !== WHATSAPP_SOURCE_ID,
      ),
    ];
    for (const source of sources) {
      expect(covered, source).toContain(source);
    }
  });

  it.each(FORM_CASES)("marks exactly the fields Import needs: $name", ({ props, fields }) => {
    const filled = renderForm(props);
    expect(importButton(), "Import is enabled with every field filled").not.toBeDisabled();
    const marked = markedLabels();
    filled.unmount();

    const needed: string[] = [];
    for (const field of fields) {
      const emptied = renderForm({ ...props, ...field.empty });
      if (importButton().hasAttribute("disabled")) needed.push(field.label);
      emptied.unmount();
    }

    expect(marked).toEqual(needed.sort());
  });
});

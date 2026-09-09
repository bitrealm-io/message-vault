/** @vitest-environment jsdom */

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { IMESSAGE_SOURCE_ID } from "../../lib/imessageImport";
import {
  emptyWhatsappPathStats,
  WHATSAPP_ERR_CRYPT_KEY,
  WHATSAPP_ERR_FOLDER_IS_FILE,
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
    force: false,
    onForceChange: vi.fn(),
    obfuscate: false,
    onObfuscateChange: vi.fn(),
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
      },
    });
    expect(screen.getByText(WHATSAPP_ERR_CRYPT_KEY)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Import" })).toBeDisabled();
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

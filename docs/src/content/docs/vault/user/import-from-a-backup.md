---
title: Import from a backup
description: Use the desktop app Import screen to read a phone backup and store it in the vault.
---

**Import** is in the desktop app sidebar after you sign in. It is not shown in the browser-only UI. Pick a backup source, point at the file or folder, and start the run. The app extracts from that backup and pushes into the vault in one flow.

JSONL (JSON Lines) folders on disk are a command-line task: [Extract to files](/vault/user/how-to/extract-to-files/).

## Before you start

- A vault that is running — [Try the vault](/vault/user/get-started/try-the-vault/)
- The desktop app signed in as **your** account (not `demo`), server URL such as `http://localhost:8080`
- A prepared backup — [Prepare a backup](/vault/user/prepare-a-backup/)

## Run Import

1. Sign in to the vault in the desktop app
2. Open **Import** in the sidebar
3. Choose a **source** that matches the backup:

   | Source in the app | Typical files |
   |---|---|
   | **iMessage** → **Platform:** **iPhone backup** | Finder/iTunes backup folder (device UUID directory), not a `.db` file inside it |
   | **iMessage** → **Platform:** **Mac Messages** | `chat.db` |
   | **WhatsApp** → **Platform:** **Android** | Folder with `msgstore.db` or `msgstore.db.crypt*` plus key |
   | **WhatsApp** → **Platform:** **iPhone** | iPhone backup that includes WhatsApp |
   | **SMS Backup & Restore** | SyncTech XML |

   Rescue sources (GO SMS Pro, iMazing, OpenExtract, SMS Backup+) are documented under [rescue imports](/vault/user/how-to/rescue-imports/).

4. Fill in paths, passwords, keys, or owner phone numbers for that source. A red asterisk marks a field that has no default and must be filled. **(Optional)** marks an empty field you can leave blank. Dropdowns that already have a value (Platform, Attachments, Contacts) have no extra mark.
5. Start the run and watch the on-screen progress and log

### iMessage fields

After you pick **iMessage**, **Platform** chooses Mac Messages or iPhone backup.

**iPhone backup**

- **iPhone Backup Directory** (required) — the device UUID folder from Finder or iTunes, not `sms.db` inside it. See [iPhone or iPad](/vault/user/prepare-a-backup/iphone-ipad/).
- **Encryption password** — required (red asterisk) when the backup is encrypted. **(Optional)** when it is not. Fill it in only for an encrypted backup.

**Mac Messages**

- **Messages database** (required) — path to `chat.db`.
- **Attachment folder (Optional)** — leave empty when `Attachments` and `StickerCache` sit next to `chat.db` (the usual Mac layout under `~/Library/Messages`). Set this only when those folders live somewhere else, for example after copying `chat.db` on its own.
- **Apple Contacts file (Optional)** — leave empty to use the local AddressBook on a live Mac. Point at `AddressBook-v22.abcddb` or `AddressBook.sqlitedb` only if that file is not in the usual Contacts location. People do not normally move that file.

**Attachments** and **Contacts** apply to both platforms. Attachments is Copy / Convert / Compress / Skip. Contacts fills names from vault contacts after import; that is separate from the Apple Contacts file above.

### WhatsApp fields

After you pick **WhatsApp**, **Platform** chooses Android or iPhone. Default Platform is **Android**.

**Android**

- **Backup folder** (required) — a folder that contains `msgstore.db` and/or `msgstore.db.crypt12` / `.crypt14` / `.crypt15`. See [WhatsApp on Android](/vault/user/prepare-a-backup/android-whatsapp/).
- **Decryption key** — required (red asterisk) when the folder has a crypt file and no decrypted `msgstore.db`. **(Optional)** when `msgstore.db` is already in the folder. Enter a key file path or a crypt15 hex string. This is the WhatsApp Android decryption key, not the Apple backup password. The app does not save it.
- **Contacts database (Optional)** — `wa.db`. Leave empty if that file is in the backup folder.
- **Media folder (Optional)** — leave empty if a `WhatsApp` media folder is in the backup folder.
- **Message database (Optional)** — leave empty if `msgstore.db` is in the backup folder.

**iPhone**

- **Backup folder** (required) — the device UUID folder from Finder or iTunes. See [WhatsApp on iPhone](/vault/user/prepare-a-backup/iphone-whatsapp/).
- **Contacts database (Optional)** — `ContactsV2.sqlite`. Leave empty if that file is in the backup.
- **WhatsApp Business** — optional checkbox, unmarked by default. Turn it on only for a WhatsApp Business backup. The app does not remember this choice.

**Attachments** and **Contacts** apply to both platforms. Attachments is Copy / Convert / Compress / Skip. Contacts fills names from vault contacts after import; that is separate from the WhatsApp contacts database above.

## Stages and approvals

An import is one **Import Run**, and your account has at most one running at a time. It moves through three stages, and it stops to ask you before spending more time or touching the vault:

1. **Staging** reads the backup and copies its messages and original attachments into a staging folder on this computer. Nothing reaches the vault yet.
2. **Staging Approval.** The run shows what it staged, read from the files rather than estimated: conversations, messages, contacts (and how many are new to your vault), attachments and their size, and the addresses the backup sent from. **Approve** to continue, or **Cancel this import** to end the run and delete what was staged.
3. **Media** converts or compresses the staged attachments. This stage exists only when you chose **Convert** or **Compress & Convert**; under **Copy** and **Skip** the run goes straight from the Staging Approval to Upload.
4. **Media Approval.** The run shows how the converted files came out against what you approved: files that will not be uploaded after all, files that came in under the limit, and what is still flagged. Approve to upload, or cancel.
5. **Upload** writes the staged messages and attachments into the vault, then shows a summary: messages added, messages already in the vault, attachments uploaded, and every contact the run created, named, or gave a handle to, with the reason beside each. From there, open the conversations the run added or the contacts it touched, or start another import.

The Import screen collects each stage's result under **What you asked for** as the run goes, so the whole run is on one page. When a stage finishes and you are on Import, the approval opens on its own; it has a **Back to the run** link if you want to look at the run first, and **Review and approve** brings it back.

You do not have to sit and wait. A run keeps working while you read messages or edit contacts, and it keeps waiting at an approval while you are elsewhere or after you close the app. The **Import** entry in the sidebar carries a badge while a run is waiting for you or has failed. **Cancel** on the Import screen stops the stage that is running; the run stays where it got to, and the next visit to Import offers to resume or discard it.

## Resume and force reprocessing

Import writes a journal file (`.vault-import-state.jsonl`) next to the work it does. On a later run with the same vault and folder, the journal skips work that already finished.

Leave **force reprocessing** off when continuing an interrupted upload.

Turn force reprocessing on when a previous run left messages without attachments, you fixed missing files, or the local journal is wrong. The vault still deduplicates on its end — messages and attachments already stored are skipped rather than duplicated. Force reprocessing does not wipe the database.

## After the run

The finished run leads with where to go next: **Conversations this import added** opens the conversation list narrowed to the run (`import:#` followed by the run's number), and **Contacts it touched** opens the Contact Group the vault made for the run. The run's record, with the same contact list, stays under **Settings → Storage → Import history**. See [Browse your messages](/vault/user/browse-your-messages/).

API tokens under **Settings → Account** are for programs that call the vault's [HTTP API](/vault/developer/reference/api/), not for this screen. Desktop Import uses the signed-in session.

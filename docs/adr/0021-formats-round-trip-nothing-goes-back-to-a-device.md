# Formats round-trip; nothing goes back to a device

Message Crate never restores messages or data to a phone or any other device:
it has no restore flow, pushes nothing to a device, and uploads nothing to a
mail account for a vendor's app to restore from. It does write vendor formats,
so that a person can have their data back in the format it came in (#543). A
vendor writer writes only the messages its format can describe, and the Export
Run reports how many it left out. A field the database does not keep is left
out of the file, never made up.

## Why

Restoring is the vendor app's job, and each app restores on its own terms.
SMS Backup+ restores only from an IMAP folder, never from a file, and never
restores MMS; it has been off Google Play and F-Droid since 2025. Building
toward a restore would tie Message Crate to apps it does not control, while a
file in the vendor's format is useful whether or not the app still exists:
Message Crate reads it back, and for mail formats any mail program can archive
it.

A message written into a format that cannot describe it is mislabelled. A
WhatsApp chat written as SMS Backup+ mail would come back from a re-import as
SMS, under a new id. So each vendor writer keeps to its format, and the
person is told what was left out; Message Crate's own formats (JSON Lines,
JSON, CSV, the EML archive, MBOX) hold every message.

Export reads the database, and the database keeps what is common to every
source, not each vendor's own fields (a phone's row and thread ids, read
flags, an app's build). An exported file therefore says less than the backup
it came from. Inventing those values would make the file claim things that
never happened, such as a phone row id or an app build, so they are omitted.

## Considered and rejected

**Keeping every vendor field in the database** so an export reproduces the
original backup exactly. Rejected for now: it is a column on every message
for a guarantee no one has needed yet, and a loss of those fields was
accepted on 2 October 2026.

**Writing every message into every format**, as the SMS Backup & Restore
writer did. Rejected because of the mislabelling above.

## Consequences

Round-trip tests for a vendor format import a fixture, export it in that
format, and import the result again; they compare messages, not the vendor
app's exact headers. The SMS Backup & Restore writer, which wrote every
message as `<sms>` or `<mms>`, keeps to SMS and MMS under this rule.

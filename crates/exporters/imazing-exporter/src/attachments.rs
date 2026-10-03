//! Find the file iMazing wrote for a CSV row, in the row's own chat folder.
//!
//! iMazing names a media file `{Message Date} - {label} - {name}`. The date
//! is the row's `Message Date` with each `:` replaced by a space. The label
//! comes from the chat and can't be rebuilt from a row, so it is not
//! compared. The name is the row's `Attachment` cell as iMazing changed it
//! when it wrote the file ([`names_on_disk`]).

use chrono::NaiveDateTime;
use message_csv::AttachmentCell;
use std::fs;
use std::path::{Path, PathBuf};

/// The extensions iMazing converts when it writes a file, each with the one
/// it writes instead.
const CONVERTED_EXTENSIONS: [(&str, &str); 4] = [
    ("heic", "jpg"),
    ("caf", "mp3"),
    ("opus", "mp3"),
    ("webp", "png"),
];

/// The longest stem iMazing writes into a file name, in characters.
const MAX_STEM_CHARS: usize = 40;

/// The `Message Date` of a row as iMazing writes it at the start of a file
/// name: `YYYY-MM-DD HH MM SS`.
///
/// A date the CSV writes without seconds gets ` 00`, because iMazing always
/// writes the seconds into a file name. A date that does not parse has each
/// `:` replaced by a space.
pub(crate) fn file_name_second(message_date: &str) -> String {
    let raw = message_date.trim();
    NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M"))
        .map(|date| date.format("%Y-%m-%d %H %M %S").to_string())
        .unwrap_or_else(|_| raw.replace(':', " "))
}

/// The regular files directly in one chat folder.
pub(crate) struct FolderFiles {
    files: Vec<(String, PathBuf)>,
}

impl FolderFiles {
    /// Read the regular files directly in `folder`.
    ///
    /// Symbolic links are skipped, because following one can reach a file
    /// outside the export. A name that is not UTF-8 is skipped, because no
    /// CSV cell can name it.
    pub(crate) fn read(folder: &Path) -> Self {
        let mut files = Vec::new();
        if let Ok(entries) = fs::read_dir(folder) {
            for entry in entries.flatten() {
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                if file_type.is_symlink() || !file_type.is_file() {
                    continue;
                }
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    files.push((name.to_string(), path));
                }
            }
        }
        FolderFiles { files }
    }

    /// The file iMazing wrote for `row`: a name that starts with the row's
    /// second and ` - `, and ends with ` - ` and the first of
    /// [`names_on_disk`] that any file ends with.
    ///
    /// A file whose name ends with a longer name that another row of the
    /// same second gives is that row's, so it is left out: `photo.jpg` does
    /// not take `Holiday - photo.jpg`. `None` when no file has that shape, or
    /// when two or more do, because then nothing tells which file is the
    /// row's.
    pub(crate) fn find(&self, row: &RowAttachment<'_>) -> Option<PathBuf> {
        let start = format!("{} - ", row.second);
        let others: Vec<String> = row
            .same_second
            .iter()
            .filter(|&&other| other != row.name)
            .flat_map(names_on_disk)
            .map(|name| format!(" - {name}"))
            .collect();
        let fits = |file: &str, end: &str| {
            file.len() >= start.len() + end.len() && file.starts_with(&start) && file.ends_with(end)
        };
        for name in names_on_disk(&row.name) {
            let end = format!(" - {name}");
            let mut matches = self.files.iter().filter(|(file, _)| {
                fits(file, &end)
                    && !others
                        .iter()
                        .any(|other| other.len() > end.len() && fits(file, other))
            });
            if let Some((_, path)) = matches.next() {
                return matches.next().is_none().then(|| path.clone());
            }
        }
        None
    }
}

/// What a row tells about the file iMazing wrote for its attachment.
pub(crate) struct RowAttachment<'a> {
    /// The row's `Attachment` cell and its number.
    pub name: NumberedName<'a>,
    /// The row's `Message Date` as iMazing writes it into a file name
    /// ([`file_name_second`]).
    pub second: &'a str,
    /// The [`NumberedName`] of every row of the CSV at this row's second,
    /// this row's among them.
    pub same_second: &'a [NumberedName<'a>],
}

/// A row's `Attachment` cell and its place among the rows of its CSV that
/// share its second and [`written_name`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct NumberedName<'a> {
    /// The row's `Attachment` cell, a bare basename.
    pub csv_name: &'a str,
    /// Which of those rows this row is, counting from 1 in CSV order.
    pub ordinal: usize,
}

/// The name iMazing writes for a file whose `Attachment` cell is `csv_name`:
/// the stem with every non-ASCII character removed and cut to 40
/// characters, and the extension converted ([`CONVERTED_EXTENSIONS`]).
///
/// Rows of one second whose cells give one written name are numbered
/// together (` 2`, ` 3`), because one folder can't hold two files of one
/// name.
pub(crate) fn written_name(csv_name: &str) -> String {
    let (stem, extension) = split_name(csv_name);
    let extension = extension.map(|extension| converted_extension(extension).unwrap_or(extension));
    with_ordinal(&short_stem(stem), extension, 1)
}

/// The names iMazing may have given the file of a row whose `Attachment`
/// cell is `csv_name`, most likely first:
///
/// 1. the name as written;
/// 2. the name with the extension converted ([`CONVERTED_EXTENSIONS`]);
/// 3. either of these with every non-ASCII character removed from the stem
///    and the stem cut to its first 40 characters.
///
/// When rows of one CSV share a second and a [`written_name`], iMazing writes
/// `X.ext` for the first and `X 2.ext`, `X 3.ext`, … for the rest, so the
/// `ordinal`-th row's stem ends with ` {ordinal}` from the second on.
fn names_on_disk(name: &NumberedName<'_>) -> Vec<String> {
    let NumberedName { csv_name, ordinal } = *name;
    let (stem, extension) = split_name(csv_name);
    let extensions = match extension {
        None => vec![None],
        Some(extension) => [Some(extension), converted_extension(extension)]
            .into_iter()
            .filter(Option::is_some)
            .collect(),
    };
    let short = short_stem(stem);
    let mut names: Vec<String> = Vec::new();
    for stem in [stem, short.as_str()] {
        for &extension in &extensions {
            let name = with_ordinal(stem, extension, ordinal);
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// The stem and extension of a file name. A name with no `.` after its first
/// character has no extension.
fn split_name(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, Some(extension)),
        _ => (name, None),
    }
}

/// The extension iMazing writes in place of `extension`. It is compared
/// exactly, as #1082 settles: nothing measured shows what iMazing writes
/// for an upper-case one.
fn converted_extension(extension: &str) -> Option<&'static str> {
    CONVERTED_EXTENSIONS
        .iter()
        .find(|(from, _)| extension == *from)
        .map(|(_, to)| *to)
}

/// `stem` with every non-ASCII character removed, cut to its first 40
/// characters.
fn short_stem(stem: &str) -> String {
    stem.chars()
        .filter(char::is_ascii)
        .take(MAX_STEM_CHARS)
        .collect()
}

/// `stem` with ` {ordinal}` after it from the second row on, then the extension.
fn with_ordinal(stem: &str, extension: Option<&str>, ordinal: usize) -> String {
    let mut name = stem.to_string();
    if ordinal > 1 {
        name.push_str(&format!(" {ordinal}"));
    }
    if let Some(extension) = extension {
        name.push('.');
        name.push_str(extension);
    }
    name
}

/// Inputs for [`resolve_attachment_cell`].
pub(crate) struct ResolveAttachmentArgs<'a> {
    pub row: RowAttachment<'a>,
    pub attachment_type: &'a str,
    /// The files of the row's chat folder. `None` when the run does not copy
    /// attachments, so no file is looked for.
    pub files: Option<&'a FolderFiles>,
}

/// Resolve a CSV attachment name into an [`AttachmentCell`] and the file
/// iMazing wrote for the row, when the row's chat folder holds it.
///
/// Does not copy files. A row with no file keeps the CSV name, and the
/// writer marks its attachment `file_missing`.
pub(crate) fn resolve_attachment_cell(
    args: ResolveAttachmentArgs<'_>,
) -> (AttachmentCell, Option<PathBuf>) {
    let ResolveAttachmentArgs {
        row,
        attachment_type,
        files,
    } = args;
    let cell = AttachmentCell {
        meta: message_ir::AttachmentMeta {
            path: None,
            original_name: Some(row.name.csv_name.to_string()),
            mime_type: mime_hint(attachment_type, row.name.csv_name),
            digest_sha256: None,
            size_bytes: None,
            missing_reason: None,
        },
        is_sticker: attachment_type.eq_ignore_ascii_case("sticker"),
        transcription: None,
        sticker_effect: None,
    };
    let source = files.and_then(|files| files.find(&row));
    (cell, source)
}

/// The MIME type of an attachment, from its `Attachment type` cell or, when
/// that is empty, from the file name's extension.
pub(crate) fn mime_hint(attachment_type: &str, filename: &str) -> Option<String> {
    let t = attachment_type.trim().to_ascii_lowercase();
    if !t.is_empty() {
        return Some(match t.as_str() {
            "image" => "image/jpeg".into(),
            "video" => "video/mp4".into(),
            "audio" => "audio/mpeg".into(),
            "gif" => "image/gif".into(),
            "sticker" => "image/webp".into(),
            other => other.to_string(),
        });
    }
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".png") {
        Some("image/png".into())
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some("image/jpeg".into())
    } else if lower.ends_with(".gif") {
        Some("image/gif".into())
    } else if lower.ends_with(".heic") {
        Some("image/heic".into())
    } else if lower.ends_with(".mp4") || lower.ends_with(".mov") {
        Some("video/mp4".into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use message_crate_core::{AttachmentJob, MediaConfig, run_attachment_jobs};
    use message_ir::IrAttachment;

    /// The attachment-type column names the type when iMazing filled it in;
    /// when it is empty, the file name's extension does, and an extension
    /// nobody knows gives no type rather than a wrong one.
    #[test]
    fn the_mime_type_comes_from_the_column_or_else_the_file_name() {
        for (column, mime) in [
            ("Image", "image/jpeg"),
            ("video", "video/mp4"),
            ("Audio", "audio/mpeg"),
            ("GIF", "image/gif"),
            ("sticker", "image/webp"),
            ("image/png", "image/png"),
        ] {
            assert_eq!(
                mime_hint(column, "IMG_0001.heic").as_deref(),
                Some(mime),
                "{column}"
            );
        }
        for (name, mime) in [
            ("IMG_0001.PNG", Some("image/png")),
            ("IMG_0001.jpg", Some("image/jpeg")),
            ("IMG_0001.jpeg", Some("image/jpeg")),
            ("IMG_0001.gif", Some("image/gif")),
            ("IMG_0001.heic", Some("image/heic")),
            ("IMG_0001.mp4", Some("video/mp4")),
            ("IMG_0001.MOV", Some("video/mp4")),
            ("notes.xyz", None),
        ] {
            assert_eq!(mime_hint(" ", name).as_deref(), mime, "{name}");
        }
    }

    /// A row of `2020-01-01 12:00:00` naming `csv_name`, the first of its name
    /// in that second.
    fn row(csv_name: &str) -> RowAttachment<'_> {
        RowAttachment {
            name: NumberedName {
                csv_name,
                ordinal: 1,
            },
            second: "2020-01-01 12 00 00",
            same_second: &[],
        }
    }

    #[test]
    fn copied_attachment_includes_digest() {
        let dir = tempfile::tempdir().unwrap();
        let chat = dir.path().join("chat");
        let attachments = dir.path().join("attachments");
        fs::create_dir_all(&chat).unwrap();
        fs::create_dir_all(&attachments).unwrap();
        fs::write(
            chat.join("2020-01-01 12 00 00 - Bob - photo.jpg"),
            b"jpeg-bytes",
        )
        .unwrap();
        let files = FolderFiles::read(&chat);
        let (cell, source) = resolve_attachment_cell(ResolveAttachmentArgs {
            row: row("photo.jpg"),
            attachment_type: "image",
            files: Some(&files),
        });
        assert!(
            cell.meta.digest_sha256.is_none(),
            "resolve must not hash or write"
        );
        assert!(cell.meta.path.is_none());
        assert!(
            fs::read_dir(&attachments).unwrap().next().is_none(),
            "resolve must not write files"
        );
        let source = source.expect("source path found");
        let mut att = IrAttachment {
            path: None,
            original_name: cell.meta.original_name,
            mime_type: cell.meta.mime_type,
            digest_sha256: None,
            is_sticker: cell.is_sticker,
            transcription: None,
            sticker_effect: None,
            size_bytes: None,
            missing_reason: None,
            bytes: None,
        };
        let mut jobs = [AttachmentJob {
            attachment: &mut att,
            timestamp_unix_ms: 1_600_000_000_000,
            size_hint: None,
        }];
        run_attachment_jobs(
            &mut jobs,
            &attachments,
            &MediaConfig::default(),
            |_| fs::read(&source).map(Some).or(Ok(None)),
            |_| {},
            None,
            None,
        )
        .unwrap();
        let digest = att.digest_sha256.expect("digest set after runner");
        assert_eq!(digest.len(), 64);
        assert!(att.path.as_deref().unwrap().starts_with("attachments/"));
        assert_eq!(fs::read_dir(&attachments).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_in_the_chat_folder_is_not_followed() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let chat = dir.path().join("chat");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&chat).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.jpg"), b"secret").unwrap();
        symlink(
            outside.join("secret.jpg"),
            chat.join("2020-01-01 12 00 00 - Bob - photo.jpg"),
        )
        .unwrap();
        assert_eq!(FolderFiles::read(&chat).find(&row("photo.jpg")), None);
    }

    fn numbered(csv_name: &str, ordinal: usize) -> NumberedName<'_> {
        NumberedName { csv_name, ordinal }
    }

    /// The candidate names in order: as written, converted, then each with
    /// the stem made ASCII and cut to 40 characters. A name iMazing would
    /// leave as it is gives only itself.
    #[test]
    fn the_names_on_disk_are_tried_as_written_first() {
        assert_eq!(
            names_on_disk(&numbered("IMG_0001.jpg", 1)),
            vec!["IMG_0001.jpg"]
        );
        assert_eq!(
            names_on_disk(&numbered("IMG_0001.heic", 1)),
            vec!["IMG_0001.heic", "IMG_0001.jpg"]
        );
        assert_eq!(
            names_on_disk(&numbered("IMG_0001.HEIC", 1)),
            vec!["IMG_0001.HEIC"]
        );
        assert_eq!(
            names_on_disk(&numbered("Caf\u{e9} \u{2019}menu\u{2019}.webp", 2)),
            vec![
                "Caf\u{e9} \u{2019}menu\u{2019} 2.webp",
                "Caf\u{e9} \u{2019}menu\u{2019} 2.png",
                "Caf menu 2.webp",
                "Caf menu 2.png",
            ]
        );
        assert_eq!(
            names_on_disk(&numbered("sticker_0001", 3)),
            vec!["sticker_0001 3"]
        );
    }

    /// Two cells that iMazing writes as one name are numbered together.
    #[test]
    fn the_written_name_is_the_name_after_every_change() {
        assert_eq!(written_name("IMG_0001.heic"), "IMG_0001.jpg");
        assert_eq!(written_name("IMG_0001.jpg"), "IMG_0001.jpg");
        assert_eq!(written_name("Caf\u{e9}.pdf"), "Caf.pdf");
        assert_eq!(written_name("sticker_0001"), "sticker_0001");
        assert_eq!(
            written_name("Minutes of the neighbourhood garden committee meeting.pdf"),
            "Minutes of the neighbourhood garden comm.pdf"
        );
    }

    /// iMazing writes the seconds into every file name, also for a row whose
    /// `Message Date` has none.
    #[test]
    fn the_file_name_second_always_has_seconds() {
        assert_eq!(
            file_name_second("2020-01-01 12:01:00"),
            "2020-01-01 12 01 00"
        );
        assert_eq!(
            file_name_second(" 2020-01-01 12:01 "),
            "2020-01-01 12 01 00"
        );
        assert_eq!(file_name_second("not a date"), "not a date");
    }
}

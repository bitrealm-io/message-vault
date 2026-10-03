//! Find the file iMazing wrote for a CSV row, in the row's own chat folder.
//!
//! iMazing names a media file `{Message Date} - {label} - {name}`. The date
//! is the row's `Message Date` with each `:` replaced by a space. The label
//! comes from the chat and can't be rebuilt from a row, so it is not
//! compared. The name is the row's `Attachment` cell as iMazing changed it
//! when it wrote the file ([`names_on_disk`]).

use crate::parse::RawRow;
use chrono::NaiveDateTime;
use message_csv::AttachmentCell;
use std::collections::HashMap;
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
/// writes the seconds into a file name. `None` for a date that does not
/// parse, because the row of such a date is not imported.
pub(crate) fn file_name_second(message_date: &str) -> Option<String> {
    let raw = message_date.trim();
    NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M"))
        .map(|date| date.format("%Y-%m-%d %H %M %S").to_string())
        .ok()
}

/// The length of a [`file_name_second`] in bytes.
const SECOND_LEN: usize = "YYYY-MM-DD HH MM SS".len();

/// The separator iMazing writes between the parts of a media file's name.
const SEPARATOR: &str = " - ";

/// The regular files directly in one chat folder, by the second their name
/// starts with.
pub(crate) struct FolderFiles {
    by_second: HashMap<String, SecondFiles>,
}

/// The files of one chat folder whose names start with one second and
/// ` - `.
#[derive(Default)]
struct SecondFiles {
    /// Each file's path, and every name the file's name ends with after a
    /// ` - ` that follows the second's ` - `.
    files: Vec<(PathBuf, Vec<String>)>,
    /// The files, by each name their name ends with (indices into `files`).
    by_name: HashMap<String, Vec<usize>>,
}

impl FolderFiles {
    /// Read the regular files directly in `folder`.
    ///
    /// Symbolic links are skipped, because following one can reach a file
    /// outside the export. A name that is not UTF-8 is skipped, because no
    /// CSV cell can name it. A name that does not start with a second and
    /// ` - ` is skipped, because iMazing wrote it for no row.
    pub(crate) fn read(folder: &Path) -> Self {
        let mut by_second: HashMap<String, SecondFiles> = HashMap::new();
        let Ok(entries) = fs::read_dir(folder) else {
            return FolderFiles { by_second };
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() || !file_type.is_file() {
                continue;
            }
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(second) = name.get(..SECOND_LEN) else {
                continue;
            };
            let label_start = SECOND_LEN + SEPARATOR.len();
            if name.get(SECOND_LEN..label_start) != Some(SEPARATOR) {
                continue;
            }
            // Every ` - ` at or after the label's start can begin the name
            // part, because the label may be empty and the name may hold
            // ` - ` itself. Two of them can overlap, as in ` - - ` when the
            // label ends with ` -`, so each byte is a possible start. The
            // separator is ASCII, so the name after it starts on a character.
            let ends: Vec<String> = (label_start..name.len())
                .filter(|&at| name.as_bytes()[at..].starts_with(SEPARATOR.as_bytes()))
                .map(|at| name[at + SEPARATOR.len()..].to_string())
                .collect();
            let second_files = by_second.entry(second.to_string()).or_default();
            let index = second_files.files.len();
            for end in &ends {
                second_files
                    .by_name
                    .entry(end.clone())
                    .or_default()
                    .push(index);
            }
            second_files.files.push((path, ends));
        }
        FolderFiles { by_second }
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
    fn find(&self, row: &RowAttachment<'_>) -> Option<&Path> {
        let second_files = self.by_second.get(row.second)?;
        let taken_by_other = |ends: &[String], name: &str| {
            ends.iter().any(|end| {
                end.len() > name.len()
                    && row
                        .same_second
                        .get(end)
                        .is_some_and(|owners| owners.iter().any(|&owner| owner != row.name))
            })
        };
        for name in names_on_disk(&row.name) {
            let Some(indices) = second_files.by_name.get(&name) else {
                continue;
            };
            let mut matches = indices
                .iter()
                .map(|&index| &second_files.files[index])
                .filter(|(_, ends)| !taken_by_other(ends, &name));
            if let Some((path, _)) = matches.next() {
                return matches.next().is_none().then_some(path.as_path());
            }
        }
        None
    }
}

/// The rows of one second that name a file, by each name iMazing may have
/// given their files ([`names_on_disk`]).
type NamesAtSecond<'a> = HashMap<String, Vec<NumberedName<'a>>>;

/// The file iMazing wrote for each row of one CSV, in the CSV's order, from
/// the files of the CSV's chat folder. `seconds` holds each row's
/// [`file_name_second`], in the same order. `None` for a row that names no
/// file or whose date does not parse, and for a row whose file can't be told
/// apart from another row's:
///
/// - rows of one second and one numbering name ([`numbering_name`]) whose
///   files are not all in the folder get none, because a missing file moves
///   the ` 2`, ` 3` numbers and nothing tells which row's file is gone;
/// - a file that two or more rows would take goes to none of them, because
///   one file is never two rows' attachment.
pub(crate) fn row_sources(
    rows: &[RawRow],
    seconds: &[Option<String>],
    files: &FolderFiles,
) -> Vec<Option<PathBuf>> {
    // Each row's numbering group and its place in it, counting from 1.
    let mut groups: HashMap<(&str, String), Vec<usize>> = HashMap::new();
    let mut named: Vec<Option<(&str, NumberedName<'_>)>> = Vec::with_capacity(rows.len());
    for (index, (row, second)) in rows.iter().zip(seconds).enumerate() {
        let Some(second) = second.as_deref().filter(|_| !row.attachment.is_empty()) else {
            named.push(None);
            continue;
        };
        let group = groups
            .entry((second, numbering_name(&row.attachment)))
            .or_default();
        group.push(index);
        named.push(Some((
            second,
            NumberedName {
                csv_name: &row.attachment,
                ordinal: group.len(),
            },
        )));
    }
    let mut by_second: HashMap<&str, NamesAtSecond<'_>> = HashMap::new();
    for &(second, name) in named.iter().flatten() {
        let names = by_second.entry(second).or_default();
        for name_on_disk in names_on_disk(&name) {
            names.entry(name_on_disk).or_default().push(name);
        }
    }
    let mut sources: Vec<Option<&Path>> = named
        .iter()
        .map(|named| {
            let (second, name) = (*named)?;
            files.find(&RowAttachment {
                name,
                second,
                same_second: by_second.get(second)?,
            })
        })
        .collect();
    for group in groups.values() {
        if group.iter().any(|&index| sources[index].is_none()) {
            for &index in group {
                sources[index] = None;
            }
        }
    }
    let mut takers: HashMap<&Path, usize> = HashMap::new();
    for source in sources.iter().flatten() {
        *takers.entry(source).or_default() += 1;
    }
    sources
        .iter()
        .map(|source| {
            source
                .filter(|source| takers[source] == 1)
                .map(Path::to_path_buf)
        })
        .collect()
}

/// What a row tells about the file iMazing wrote for its attachment.
struct RowAttachment<'a> {
    /// The row's `Attachment` cell and its number.
    name: NumberedName<'a>,
    /// The row's `Message Date` as iMazing writes it into a file name
    /// ([`file_name_second`]).
    second: &'a str,
    /// Every row of the CSV at this row's second, this row among them, by
    /// each name iMazing may have given its file.
    same_second: &'a NamesAtSecond<'a>,
}

/// A row's `Attachment` cell and its place among the rows of its CSV that
/// share its second and [`numbering_name`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct NumberedName<'a> {
    /// The row's `Attachment` cell, a bare basename.
    csv_name: &'a str,
    /// Which of those rows this row is, counting from 1 in CSV order.
    ordinal: usize,
}

/// The stem and extension iMazing writes for a file whose `Attachment` cell
/// is `csv_name`: the stem with every non-ASCII character removed and cut to
/// 40 characters, and the extension converted ([`CONVERTED_EXTENSIONS`]).
fn written_parts(csv_name: &str) -> (String, Option<&str>) {
    let (stem, extension) = split_name(csv_name);
    let extension = extension.map(|extension| converted_extension(extension).unwrap_or(extension));
    (short_stem(stem), extension)
}

/// The name by which rows of one second are numbered together (` 2`, ` 3`):
/// the name iMazing writes ([`written_parts`]) in lower case. iMazing
/// numbers two names that differ only in case, because a file system that
/// ignores case, as macOS and Windows do by default, can't hold both in one
/// folder.
fn numbering_name(csv_name: &str) -> String {
    let (stem, extension) = written_parts(csv_name);
    with_ordinal(&stem, extension, 1).to_lowercase()
}

/// The names iMazing may have given the file of a row whose `Attachment`
/// cell is `csv_name`, most likely first:
///
/// 1. the name as written;
/// 2. the name with the extension converted ([`CONVERTED_EXTENSIONS`]);
/// 3. either of these with every non-ASCII character removed from the stem
///    and the stem cut to its first 40 characters.
///
/// When rows of one CSV share a second and a [`numbering_name`], iMazing
/// writes `X.ext` for the first and `X 2.ext`, `X 3.ext`, … for the rest, so
/// the `ordinal`-th row's stem ends with ` {ordinal}` from the second on.
fn names_on_disk(name: &NumberedName<'_>) -> Vec<String> {
    let NumberedName { csv_name, ordinal } = *name;
    let (stem, extension) = split_name(csv_name);
    let (short, converted) = written_parts(csv_name);
    let mut names: Vec<String> = Vec::new();
    for stem in [stem, short.as_str()] {
        for extension in [extension, converted] {
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

/// The [`AttachmentCell`] of a row whose `Attachment` cell is `csv_name`.
///
/// Does not look for or copy a file ([`row_sources`] finds it). A row with
/// no file keeps the CSV name, and the writer marks its attachment
/// `file_missing`.
pub(crate) fn attachment_cell(csv_name: &str, attachment_type: &str) -> AttachmentCell {
    AttachmentCell {
        meta: message_ir::AttachmentMeta {
            path: None,
            original_name: Some(csv_name.to_string()),
            mime_type: mime_hint(attachment_type, csv_name),
            digest_sha256: None,
            size_bytes: None,
            missing_reason: None,
        },
        is_sticker: attachment_type.eq_ignore_ascii_case("sticker"),
        transcription: None,
        sticker_effect: None,
    }
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
            same_second: Box::leak(Box::default()),
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
        let cell = attachment_cell("photo.jpg", "image");
        let source = files.find(&row("photo.jpg"));
        assert!(
            cell.meta.digest_sha256.is_none(),
            "building the cell must not hash or write"
        );
        assert!(cell.meta.path.is_none());
        assert!(
            fs::read_dir(&attachments).unwrap().next().is_none(),
            "finding the file must not write files"
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
            |_| fs::read(source).map(Some).or(Ok(None)),
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

    /// Two cells that iMazing writes as one name, ignoring case, are
    /// numbered together.
    #[test]
    fn the_numbering_name_is_the_name_after_every_change_in_lower_case() {
        assert_eq!(numbering_name("IMG_0001.heic"), "img_0001.jpg");
        assert_eq!(numbering_name("IMG_0001.JPG"), "img_0001.jpg");
        assert_eq!(numbering_name("Caf\u{e9}.pdf"), "caf.pdf");
        assert_eq!(numbering_name("sticker_0001"), "sticker_0001");
        assert_eq!(
            numbering_name("Minutes of the neighbourhood garden committee meeting.pdf"),
            "minutes of the neighbourhood garden comm.pdf"
        );
    }

    /// iMazing writes the seconds into every file name, also for a row whose
    /// `Message Date` has none.
    #[test]
    fn the_file_name_second_always_has_seconds() {
        assert_eq!(
            file_name_second("2020-01-01 12:01:00").as_deref(),
            Some("2020-01-01 12 01 00")
        );
        assert_eq!(
            file_name_second(" 2020-01-01 12:01 ").as_deref(),
            Some("2020-01-01 12 01 00")
        );
        assert_eq!(file_name_second("not a date"), None);
    }
}

//! Files in a chat folder that no CSV row names.
//!
//! iMazing writes two kinds of file into a Messages chat folder without a row
//! for them: the video of a Live Photo, beside its picture, and the link
//! preview of a message that holds a link. Neither appears in any
//! `Attachment` cell, so the row lookup in [`crate::attachments`] never sees
//! them. This module sorts each such file into what it is, so the emitter can
//! attach the video to its picture's message and count the rest.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Extensions of the picture a Live Photo video sits beside, as iMazing
/// writes them.
const LIVE_PHOTO_PICTURE_EXTENSIONS: [&str; 2] = ["jpg", "jpeg"];

/// A link preview holds one `[InternetShortcut]` section of a few dozen
/// bytes. A larger `.url` file is not one, and is not read whole.
const MAX_LINK_PREVIEW_BYTES: u64 = 64 * 1024;

/// What a file that no row names is.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UnnamedFile {
    /// The video of a Live Photo: `<date> - <label> - <stem>.mov` beside the
    /// picture `<date> - <label> - <stem>.jpg` that an Image row names.
    LivePhotoVideo { video: PathBuf, picture: PathBuf },
    /// A link preview (`.url`) whose address a message of the file's second
    /// already shows, so the file holds nothing the message does not.
    LinkPreview,
    /// Any other file.
    Other,
}

/// What the rows of one chat folder name and say.
pub(crate) struct FolderRows<'a> {
    /// Every file any row of the export was matched to.
    pub named: &'a HashSet<PathBuf>,
    /// Every picture an Image row was matched to, with those rows.
    pub pictures: &'a HashMap<PathBuf, Vec<usize>>,
    /// The texts of this folder's rows, keyed by the row's `Message Date`
    /// as iMazing writes it into a file name ([`crate::attachments::file_name_second`]).
    pub texts_at: &'a HashMap<String, Vec<String>>,
}

/// Sort every regular file in `folder` that no row names, in name order.
///
/// CSV files are the folder's own exports, and a name starting with `.` is
/// the file system's (`.DS_Store`), so neither is a file iMazing wrote for a
/// message. Symbolic links are skipped, as the row lookup skips them.
pub(crate) fn unnamed_files(folder: &Path, rows: &FolderRows<'_>) -> Vec<UnnamedFile> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
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
        if name.starts_with('.') || has_extension(&path, &["csv"]) || rows.named.contains(&path) {
            continue;
        }
        files.push((name.to_string(), path));
    }
    files.sort();
    files
        .into_iter()
        .map(|(name, path)| classify(&name, path, rows))
        .collect()
}

fn classify(name: &str, path: PathBuf, rows: &FolderRows<'_>) -> UnnamedFile {
    if has_extension(&path, &["mov"])
        && let Some(picture) = live_photo_picture(&path, rows)
    {
        return UnnamedFile::LivePhotoVideo {
            video: path,
            picture,
        };
    }
    if has_extension(&path, &["url"])
        && let Some(address) = link_preview_address(&path)
        && let Some(second) = message_second(name)
        && rows
            .texts_at
            .get(second)
            .is_some_and(|texts| texts.iter().any(|text| text.contains(address.as_str())))
    {
        return UnnamedFile::LinkPreview;
    }
    UnnamedFile::Other
}

/// The picture beside `video` under the same name with a picture extension,
/// when an Image row names it.
fn live_photo_picture(video: &Path, rows: &FolderRows<'_>) -> Option<PathBuf> {
    let stem = video.file_stem()?.to_str()?;
    let folder = video.parent()?;
    LIVE_PHOTO_PICTURE_EXTENSIONS
        .iter()
        .flat_map(|ext| [ext.to_string(), ext.to_ascii_uppercase()])
        .map(|ext| folder.join(format!("{stem}.{ext}")))
        .find(|picture| rows.pictures.contains_key(picture))
}

/// The address on the `URL=` line of an `[InternetShortcut]` file.
fn link_preview_address(path: &Path) -> Option<String> {
    if fs::metadata(path).ok()?.len() > MAX_LINK_PREVIEW_BYTES {
        return None;
    }
    let body = fs::read_to_string(path).ok()?;
    body.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        let value = value.trim();
        (key.trim().eq_ignore_ascii_case("url") && !value.is_empty()).then(|| value.to_string())
    })
}

/// The `YYYY-MM-DD HH MM SS` second iMazing writes at the start of a media
/// file's name, before ` - `.
fn message_second(name: &str) -> Option<&str> {
    let second = name.get(..19)?;
    name[19..].starts_with(" - ").then_some(second)
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_second_is_read_only_from_a_name_that_starts_with_one() {
        assert_eq!(
            message_second("2020-01-01 12 01 00 - Bob - Web link.url"),
            Some("2020-01-01 12 01 00")
        );
        assert_eq!(message_second("2020-01-01 12 01 00.url"), None);
        assert_eq!(message_second("Web link.url"), None);
        // A name shorter than a second, or with a character longer than one
        // byte where the second ends, gives none rather than a panic.
        assert_eq!(message_second("2020-01-01 12 01 0é - x"), None);
    }

    #[test]
    fn the_address_is_the_url_line_of_the_shortcut() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Web link.url");
        fs::write(
            &path,
            "[InternetShortcut]\r\nURL=https://example.com/a?b=c\r\n",
        )
        .unwrap();
        assert_eq!(
            link_preview_address(&path).as_deref(),
            Some("https://example.com/a?b=c")
        );
        fs::write(&path, "[InternetShortcut]\n").unwrap();
        assert_eq!(link_preview_address(&path), None);
    }
}

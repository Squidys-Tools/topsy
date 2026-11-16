//! Transform, after Droppy's: which actions suit the files in the panel, what the panel says
//! about them, and the names the results and renamed files get.

use std::path::{Path, PathBuf};

/// How many numbered names to try before giving up on finding a free one.
const MAX_TRIES: u32 = 999;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Image,
    Video,
    Audio,
    Pdf,
    Other,
}

impl Kind {
    pub fn of(path: &Path) -> Kind {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "jpg" | "jpeg" | "jfif" | "png" | "webp" | "gif" | "bmp" | "tif" | "tiff" | "ico"
            | "heic" | "heif" | "avif" => Kind::Image,
            "mp4" | "m4v" | "mov" | "mkv" | "avi" | "wmv" | "webm" | "3gp" => Kind::Video,
            "mp3" | "m4a" | "aac" | "wav" | "wma" | "flac" => Kind::Audio,
            "pdf" => Kind::Pdf,
            _ => Kind::Other,
        }
    }

    fn noun(self, n: usize) -> &'static str {
        match (self, n == 1) {
            (Kind::Image, true) => "image",
            (Kind::Image, false) => "images",
            (Kind::Video, true) => "video",
            (Kind::Video, false) => "videos",
            (Kind::Audio, true) => "audio file",
            (Kind::Audio, false) => "audio files",
            (Kind::Pdf, true) => "PDF",
            (Kind::Pdf, false) => "PDFs",
            (Kind::Other, true) => "file",
            (Kind::Other, false) => "files",
        }
    }
}

/// What the panel can do, in the order its grid shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Compress,
    Convert,
    Resize,
    Trim,
    MakeGif,
    RemoveBackground,
    Rotate,
    RemoveMetadata,
    CombinePdf,
    Zip,
    Rename,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Compress => "Compress",
            Action::Convert => "Convert",
            Action::Resize => "Resize",
            Action::Trim => "Trim",
            Action::MakeGif => "Make GIF",
            Action::RemoveBackground => "Remove background",
            Action::Rotate => "Rotate",
            Action::RemoveMetadata => "Remove metadata",
            Action::CombinePdf => "Combine into PDF",
            Action::Zip => "Zip",
            Action::Rename => "Rename",
        }
    }

    /// What a result is called beside its original: `IMG_2041 (compressed).jpg`.
    pub fn suffix(self) -> &'static str {
        match self {
            Action::Compress => "compressed",
            Action::Convert => "converted",
            Action::Resize => "resized",
            Action::Trim => "trimmed",
            Action::MakeGif => "gif",
            Action::RemoveBackground => "no background",
            Action::Rotate => "rotated",
            Action::RemoveMetadata => "clean",
            Action::CombinePdf => "combined",
            Action::Zip => "zip",
            Action::Rename => "renamed",
        }
    }

    /// Whether it makes one result from all the files, rather than one per file.
    pub fn combines(self) -> bool {
        matches!(self, Action::CombinePdf | Action::Zip)
    }
}

/// The actions that suit every file in the panel.
pub fn actions(kinds: &[Kind]) -> Vec<Action> {
    use Action::*;
    let all = |k: Kind| !kinds.is_empty() && kinds.iter().all(|&x| x == k);
    let one = kinds.len() == 1;
    let mut list = if all(Kind::Image) {
        vec![
            Compress,
            Convert,
            Resize,
            RemoveBackground,
            Rotate,
            RemoveMetadata,
            CombinePdf,
        ]
    } else if all(Kind::Video) {
        let mut list = vec![Compress, Convert];
        if one {
            list.extend([Trim, MakeGif]);
        }
        list.extend([Rotate, RemoveMetadata]);
        list
    } else if all(Kind::Audio) {
        let mut list = vec![Compress, Convert];
        if one {
            list.push(Trim);
        }
        list
    } else if all(Kind::Pdf) {
        vec![Compress, Convert, Rotate, RemoveMetadata, CombinePdf]
    } else if !kinds.is_empty() && kinds.iter().all(|k| matches!(k, Kind::Image | Kind::Pdf)) {
        vec![CombinePdf]
    } else {
        Vec::new()
    };
    if !kinds.is_empty() {
        list.extend([Zip, Rename]);
    }
    list
}

/// The panel's subtitle: `3 images, 6.2 MB`, `2 files, 14 MB`.
pub fn summary(kinds: &[Kind], bytes: u64) -> String {
    let n = kinds.len();
    let noun = match kinds.first() {
        Some(&first) if kinds.iter().all(|&k| k == first) => first.noun(n),
        _ => Kind::Other.noun(n),
    };
    format!("{n} {noun}, {}", size(bytes))
}

/// A size as Finder shows it: `3 MB`, `1.6 MB`, `820 KB`, decimal units.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64 / 1000.0;
    let mut unit = 0;
    while value >= 999.95 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    let text = if value >= 10.0 || unit == 0 {
        format!("{value:.0}")
    } else {
        let text = format!("{value:.1}");
        text.strip_suffix(".0").map(str::to_owned).unwrap_or(text)
    };
    format!("{text} {}", UNITS[unit])
}

/// Where a result of `action` on `source` may go, best first: `photo (compressed).jpg`, then
/// `photo (compressed 2).jpg` and so on. `ext` is the result's extension, without the dot.
/// `None` for a path with no folder to put it in.
pub fn result_paths(
    source: &Path,
    action: Action,
    ext: &str,
) -> Option<impl Iterator<Item = PathBuf>> {
    let parent = source.parent()?.to_path_buf();
    let stem = source.file_stem()?.to_string_lossy().into_owned();
    let suffix = action.suffix();
    let ext = ext.to_owned();
    Some((1..=MAX_TRIES).map(move |n| {
        let tag = if n == 1 {
            suffix.to_owned()
        } else {
            format!("{suffix} {n}")
        };
        parent.join(format!("{stem} ({tag}).{ext}"))
    }))
}

/// Where the `page`th (from 0) of several results from one source goes: `report (page 3).png`.
#[allow(dead_code)] // For PDF to images, wired with the image engine.
pub fn page_paths(source: &Path, page: usize, ext: &str) -> Option<impl Iterator<Item = PathBuf>> {
    let parent = source.parent()?.to_path_buf();
    let stem = source.file_stem()?.to_string_lossy().into_owned();
    let ext = ext.to_owned();
    Some((1..=MAX_TRIES).map(move |n| {
        let again = if n == 1 {
            String::new()
        } else {
            format!(" {n}")
        };
        parent.join(format!("{stem} (page {}{again}).{ext}", page + 1))
    }))
}

/// Where one result made from all the files goes: beside the first, named after it when there
/// is one file and `name` otherwise (`Combined.pdf`, `Archive.zip`), numbered like Explorer's
/// copies (`Archive (2).zip`).
pub fn combined_paths(
    sources: &[PathBuf],
    name: &str,
    ext: &str,
) -> Option<impl Iterator<Item = PathBuf>> {
    let first = sources.first()?;
    let parent = first.parent()?.to_path_buf();
    let stem = if sources.len() == 1 {
        first.file_stem()?.to_string_lossy().into_owned()
    } else {
        name.to_owned()
    };
    let ext = ext.to_owned();
    Some((1..=MAX_TRIES).map(move |n| {
        let again = if n == 1 {
            String::new()
        } else {
            format!(" ({n})")
        };
        parent.join(format!("{stem}{again}.{ext}"))
    }))
}

/// Why a new name can't be used, or `None` if it can.
pub fn name_problem(name: &str) -> Option<&'static str> {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Some("Type a name");
    }
    if let Some(c) = name
        .chars()
        .find(|&c| "\\/:*?\"<>|".contains(c) || c.is_control())
    {
        return Some(if c.is_control() {
            "A name can't hold control characters"
        } else {
            "A name can't contain \\ / : * ? \" < > |"
        });
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Some("A name can't end with a dot or a space");
    }
    let base = trimmed.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&base.as_str()) {
        return Some("Windows keeps that name for itself");
    }
    None
}

/// The new file names for a rename: one file takes `name` as typed with its own extension;
/// several are numbered in order, `name 1.jpg`, `name 2.jpg`, starting at `start`, padded to
/// the same width so they sort right (`name 01` .. `name 12`).
pub fn rename_targets(sources: &[PathBuf], name: &str, start: u32) -> Vec<String> {
    let name = name.trim();
    let ext = |p: &PathBuf| {
        p.extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default()
    };
    if sources.len() == 1 {
        return vec![format!("{name}{}", ext(&sources[0]))];
    }
    let last = start as usize + sources.len() - 1;
    let width = last.to_string().len();
    sources
        .iter()
        .enumerate()
        .map(|(i, p)| format!("{name} {:0width$}{}", start as usize + i, ext(p)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|n| PathBuf::from(r"C:\in").join(n))
            .collect()
    }

    #[test]
    fn kinds_come_from_the_extension_in_any_case() {
        assert_eq!(Kind::of(Path::new("a.JPG")), Kind::Image);
        assert_eq!(Kind::of(Path::new("a.heic")), Kind::Image);
        assert_eq!(Kind::of(Path::new("clip.MOV")), Kind::Video);
        assert_eq!(Kind::of(Path::new("song.flac")), Kind::Audio);
        assert_eq!(Kind::of(Path::new("doc.pdf")), Kind::Pdf);
        assert_eq!(Kind::of(Path::new("notes")), Kind::Other);
    }

    #[test]
    fn the_actions_follow_the_files() {
        use Action::*;
        let images = actions(&[Kind::Image; 3]);
        assert_eq!(
            images,
            [
                Compress,
                Convert,
                Resize,
                RemoveBackground,
                Rotate,
                RemoveMetadata,
                CombinePdf,
                Zip,
                Rename
            ],
            "Droppy's grid for images"
        );
        assert!(actions(&[Kind::Video]).contains(&Trim));
        assert!(actions(&[Kind::Video]).contains(&MakeGif));
        assert!(
            !actions(&[Kind::Video; 2]).contains(&Trim),
            "trimming is per clip"
        );
        assert_eq!(
            actions(&[Kind::Image, Kind::Pdf]),
            [CombinePdf, Zip, Rename]
        );
        assert_eq!(actions(&[Kind::Image, Kind::Video]), [Zip, Rename]);
        assert_eq!(actions(&[Kind::Other]), [Zip, Rename]);
        assert!(actions(&[]).is_empty());
    }

    #[test]
    fn the_summary_counts_and_sizes_like_finder() {
        assert_eq!(summary(&[Kind::Image; 3], 6_200_000), "3 images, 6.2 MB");
        assert_eq!(summary(&[Kind::Video], 48_000_000), "1 video, 48 MB");
        assert_eq!(
            summary(&[Kind::Image, Kind::Pdf], 999),
            "2 files, 999 bytes"
        );
        assert_eq!(size(3_000_000), "3 MB");
        assert_eq!(size(1_640_000), "1.6 MB");
        assert_eq!(size(820_400), "820 KB");
        assert_eq!(size(999_990), "1 MB");
        assert_eq!(size(2_500_000_000), "2.5 GB");
    }

    #[test]
    fn results_sit_beside_the_original_and_never_clash() {
        let mut names = result_paths(Path::new(r"C:\in\IMG_2041.jpg"), Action::Compress, "jpg")
            .unwrap()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned());
        assert_eq!(names.next().unwrap(), "IMG_2041 (compressed).jpg");
        assert_eq!(names.next().unwrap(), "IMG_2041 (compressed 2).jpg");
        assert_eq!(
            page_paths(Path::new(r"C:\in\r.pdf"), 2, "png")
                .unwrap()
                .next()
                .unwrap(),
            PathBuf::from(r"C:\in\r (page 3).png")
        );
        let combined: Vec<_> = combined_paths(&paths(&["a.jpg", "b.jpg"]), "Combined", "pdf")
            .unwrap()
            .take(2)
            .collect();
        assert_eq!(
            combined,
            paths(&["Combined.pdf", "Combined (2).pdf"]),
            "several files"
        );
        assert_eq!(
            combined_paths(&paths(&["a.jpg"]), "Combined", "pdf")
                .unwrap()
                .next()
                .unwrap(),
            PathBuf::from(r"C:\in\a.pdf"),
            "one file is named after itself"
        );
        assert!(result_paths(Path::new(r"C:\"), Action::Zip, "zip").is_none());
    }

    #[test]
    fn renames_number_in_order_and_keep_extensions() {
        assert_eq!(
            rename_targets(&paths(&["IMG_1.jpg"]), " Trip ", 1),
            ["Trip.jpg"]
        );
        let many: Vec<_> = (0..12).map(|i| format!("{i}.png")).collect();
        let many = paths(&many.iter().map(String::as_str).collect::<Vec<_>>());
        let names = rename_targets(&many, "Trip", 1);
        assert_eq!(names[0], "Trip 01.png");
        assert_eq!(names[11], "Trip 12.png");
        assert_eq!(
            rename_targets(&paths(&["a.jpg", "b.mov"]), "x", 9),
            ["x 09.jpg", "x 10.mov"]
        );
    }

    #[test]
    fn names_windows_refuses_are_caught() {
        assert_eq!(name_problem("Trip"), None);
        assert!(name_problem("  ").is_some());
        assert!(name_problem("a/b").is_some());
        assert!(name_problem("what?").is_some());
        assert!(name_problem("dot.").is_some());
        assert!(name_problem("con").is_some());
        assert!(name_problem("COM1.txt").is_some());
        assert_eq!(name_problem("console"), None);
    }
}

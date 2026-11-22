//! The basket: the floating File Tray a jiggle brings up under the pointer, as Droppy's. What its
//! pill says about the files it holds, and when an empty one has nothing left to wait for.

use super::shelf::{Item, Kind};

/// What sort of thing a file is, as the pill counts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sort {
    Image,
    Video,
    Audio,
    Folder,
    File,
}

impl Sort {
    fn of(item: &Item) -> Sort {
        if item.kind == Kind::Dir {
            return Sort::Folder;
        }
        let ext = (item.path.extension())
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        match ext.as_deref() {
            Some(
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "heic" | "heif"
                | "avif" | "ico" | "svg" | "jxl",
            ) => Sort::Image,
            Some("mp4" | "mov" | "m4v" | "mkv" | "webm" | "avi" | "wmv") => Sort::Video,
            Some("mp3" | "wav" | "flac" | "m4a" | "aac" | "ogg" | "opus" | "wma") => Sort::Audio,
            _ => Sort::File,
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Sort::Image => "Image",
            Sort::Video => "Video",
            Sort::Audio => "Audio File",
            Sort::Folder => "Folder",
            Sort::File => "File",
        }
    }
}

/// The pill under the stack: how many, and of what when they are all one sort ("1 Image",
/// "3 Videos"), or "Items" for a mix. Empty for an empty basket.
pub fn summary(items: &[Item]) -> String {
    let Some(first) = items.first() else {
        return String::new();
    };
    let sort = Sort::of(first);
    let noun = if items.iter().all(|i| Sort::of(i) == sort) {
        sort.noun()
    } else {
        "Item"
    };
    let n = items.len();
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

/// Whether the shell's picture of the file is a picture of its content (a photo, a video's
/// frame), which fills its card, rather than an icon, which sits whole inside it.
pub fn shows_content(item: &Item) -> bool {
    matches!(Sort::of(item), Sort::Image | Sort::Video)
}

/// How many cards the stack draws: the newest file in front, up to two more peeking behind it.
pub fn stack_depth(len: usize) -> usize {
    len.min(3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn file(name: &str) -> Item {
        Item {
            path: PathBuf::from(name),
            kind: Kind::File,
            bytes: None,
        }
    }

    #[test]
    fn counts_one_sort_by_its_name() {
        assert_eq!(summary(&[file("a.png")]), "1 Image");
        assert_eq!(summary(&[file("a.PNG"), file("b.heic")]), "2 Images");
        assert_eq!(summary(&[file("a.mov"), file("b.mp4")]), "2 Videos");
        assert_eq!(summary(&[file("song.flac")]), "1 Audio File");
        assert_eq!(summary(&[file("report.pdf"), file("notes")]), "2 Files");
    }

    #[test]
    fn a_folder_is_a_folder_once_the_disk_says_so() {
        let mut dir = file(r"C:\photos.d");
        assert_eq!(summary(std::slice::from_ref(&dir)), "1 File");
        dir.kind = Kind::Dir;
        assert_eq!(summary(&[dir]), "1 Folder");
    }

    #[test]
    fn a_mix_is_items() {
        assert_eq!(summary(&[file("a.png"), file("b.pdf")]), "2 Items");
        assert_eq!(summary(&[]), "");
    }

    #[test]
    fn photos_and_videos_fill_their_card() {
        assert!(shows_content(&file("a.jpg")) && shows_content(&file("b.mkv")));
        assert!(!shows_content(&file("c.pdf")) && !shows_content(&file("d.flac")));
    }

    #[test]
    fn the_stack_shows_at_most_three() {
        assert_eq!(
            (0..6).map(stack_depth).collect::<Vec<_>>(),
            [0, 1, 2, 3, 3, 3]
        );
    }
}

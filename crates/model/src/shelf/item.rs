//! One chip on the shelf: a parked path, what it is on disk, and how the tray labels it.

use std::path::{Path, PathBuf};

/// What a chip is on disk. A dropped path starts `Unknown`: the stat runs off the
/// UI thread (a dead network share can stall it for its timeout) and
/// `Shelf::resolve_kinds` merges the answer back, so the chip never waits on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Unknown,
    File,
    Dir,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub path: PathBuf,
    pub kind: Kind,
    /// A file's size, once its stat is back.
    pub bytes: Option<u64>,
}

impl Item {
    /// The file name, or the whole path for something without one (a drive root).
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .unwrap_or(self.path.as_os_str())
            .to_string_lossy()
            .into_owned()
    }

    /// Up to four capital letters for the badge: the extension, `DIR` for a folder,
    /// `FILE` for a file with no extension, `…` until the stat comes back.
    pub fn badge_label(&self) -> String {
        match self.kind {
            Kind::Dir => return "DIR".into(),
            Kind::Unknown => return "…".into(),
            Kind::File => {}
        }
        match self.path.extension().and_then(|e| e.to_str()) {
            Some(ext) if !ext.is_empty() => ext.chars().take(4).collect::<String>().to_uppercase(),
            _ => "FILE".into(),
        }
    }

    /// The line under the name: a file's size, `Folder`, nothing until the stat comes back.
    pub fn detail(&self) -> String {
        match (self.kind, self.bytes) {
            (Kind::Dir, _) => "Folder".into(),
            (Kind::File, Some(bytes)) => crate::transform::size(bytes),
            (Kind::File, None) | (Kind::Unknown, _) => String::new(),
        }
    }

    /// A stable hue in `0.0..1.0` for the badge, so the same type always has the same colour.
    pub fn badge_hue(&self) -> f32 {
        // FNV-1a over the label: spread out, and the same on every run.
        let mut hash: u32 = 0x811c_9dc5;
        for byte in self.badge_label().bytes() {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(0x0100_0193);
        }
        (hash % 360) as f32 / 360.0
    }

    /// A file with the `.zip` extension, which the shelf offers to extract.
    pub fn is_zip(&self) -> bool {
        self.kind == Kind::File
            && (self.path.extension()).is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    }
}

/// Windows paths compare case-insensitively, accents and all.
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_labels() {
        let item = |p: &str, kind| Item {
            path: PathBuf::from(p),
            kind,
            bytes: None,
        };
        assert_eq!(item(r"C:\x\pic.png", Kind::File).badge_label(), "PNG");
        assert_eq!(item(r"C:\x\a.torrent", Kind::File).badge_label(), "TORR");
        assert_eq!(item(r"C:\x\Makefile", Kind::File).badge_label(), "FILE");
        assert_eq!(item(r"C:\x\folder", Kind::Dir).badge_label(), "DIR");
        assert_eq!(item(r"C:\x\pic.png", Kind::Unknown).badge_label(), "…");
        assert!(item(r"C:\x\a.ZIP", Kind::File).is_zip());
        assert!(!item(r"C:\x\a.zip", Kind::Dir).is_zip());
        assert!(!item(r"C:\x\zip", Kind::File).is_zip());
    }

    #[test]
    fn badge_hue_is_stable_and_in_range() {
        let a = Item {
            path: PathBuf::from(r"C:\x\a.png"),
            kind: Kind::File,
            bytes: None,
        };
        let b = Item {
            path: PathBuf::from(r"D:\y\b.PNG"),
            kind: Kind::File,
            bytes: None,
        };
        assert_eq!(a.badge_hue(), b.badge_hue());
        assert!((0.0..1.0).contains(&a.badge_hue()));
    }

    #[test]
    fn name_falls_back_to_the_path_for_a_drive_root() {
        let item = Item {
            path: PathBuf::from(r"C:\"),
            kind: Kind::Dir,
            bytes: None,
        };
        assert_eq!(item.name(), r"C:\");
    }
}

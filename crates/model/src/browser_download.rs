//! Telling a browser download from the Downloads folder's changes alone. Browsers write to a
//! partial file (Chrome and Edge `.crdownload`, Firefox `.part`) and rename it to the real name
//! when it's done; a cancelled one is deleted. Nothing says how big it will be.
//!
//! Firefox renames over an empty placeholder of the final name, and some browsers delete the
//! partial rather than rename it, so a partial that just goes ([`Update::Gone`]) is a finished
//! download only if its final name is there with something in it, which the caller looks up.

use std::collections::HashMap;
use std::time::{Duration, Instant};

const PARTIAL: [&str; 4] = [".crdownload", ".part", ".partial", ".opdownload"];
/// How often a growing download is reported.
pub const PROGRESS: Duration = Duration::from_secs(1);

/// A change in the folder, by file name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Added(String),
    Removed(String),
    Renamed { from: String, to: String },
    Modified(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    /// Started, renamed or grew: `partial` is the file to size.
    Running {
        id: u64,
        name: String,
        partial: String,
    },
    Finished {
        id: u64,
        name: String,
    },
    /// The partial went without a rename: finished if `name` is there and not empty.
    Gone {
        id: u64,
        name: String,
    },
}

#[derive(Debug)]
struct Running {
    id: u64,
    reported: Instant,
}

#[derive(Default, Debug)]
pub struct Downloads {
    /// By partial file name.
    running: HashMap<String, Running>,
    next: u64,
}

/// The name a partial file will end up with, if it is one. Chrome's first name for a download,
/// before it knows the real one, is "Unconfirmed 123456.crdownload".
pub fn final_name(file: &str) -> Option<&str> {
    let lower = file.to_ascii_lowercase();
    let ext = PARTIAL.iter().find(|ext| lower.ends_with(*ext))?;
    let stem = &file[..file.len() - ext.len()];
    (!stem.is_empty()).then_some(stem)
}

fn shown_name(partial: &str) -> String {
    match final_name(partial) {
        Some(name) if !name.starts_with("Unconfirmed ") => name.to_string(),
        _ => "Download".to_string(),
    }
}

impl Downloads {
    pub fn change(&mut self, change: Change, now: Instant) -> Option<Update> {
        match change {
            Change::Added(file) => {
                final_name(&file)?;
                Some(self.start(file, now))
            }
            // A partial already growing when topsy started is picked up here.
            Change::Modified(file) => {
                final_name(&file)?;
                let Some(running) = self.running.get_mut(&file) else {
                    return Some(self.start(file, now));
                };
                if now.duration_since(running.reported) < PROGRESS {
                    return None;
                }
                running.reported = now;
                Some(Update::Running {
                    id: running.id,
                    name: shown_name(&file),
                    partial: file,
                })
            }
            Change::Renamed { from, to } => {
                let known = self.running.remove(&from);
                if final_name(&to).is_some() {
                    let id = known.map_or_else(|| self.id(), |r| r.id);
                    self.running
                        .insert(to.clone(), Running { id, reported: now });
                    return Some(Update::Running {
                        id,
                        name: shown_name(&to),
                        partial: to,
                    });
                }
                final_name(&from)?;
                let id = known.map_or_else(|| self.id(), |r| r.id);
                Some(Update::Finished { id, name: to })
            }
            Change::Removed(file) => {
                let running = self.running.remove(&file)?;
                Some(Update::Gone {
                    id: running.id,
                    name: final_name(&file)?.to_string(),
                })
            }
        }
    }

    fn start(&mut self, file: String, now: Instant) -> Update {
        let id = self.id();
        self.running
            .insert(file.clone(), Running { id, reported: now });
        Update::Running {
            id,
            name: shown_name(&file),
            partial: file,
        }
    }

    fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> String {
        x.to_string()
    }

    #[test]
    fn partial_names() {
        assert_eq!(final_name("a.pdf.crdownload"), Some("a.pdf"));
        assert_eq!(final_name("a.pdf.PART"), Some("a.pdf"));
        assert_eq!(final_name("a.pdf"), None);
        assert_eq!(final_name(".part"), None);
        assert_eq!(shown_name("Unconfirmed 123456.crdownload"), "Download");
    }

    #[test]
    fn chrome_names_it_renames_it_and_finishes() {
        let t = Instant::now();
        let mut d = Downloads::default();
        let first = d.change(Change::Added(s("Unconfirmed 42.crdownload")), t);
        assert_eq!(
            first,
            Some(Update::Running {
                id: 1,
                name: s("Download"),
                partial: s("Unconfirmed 42.crdownload")
            })
        );
        let named = Change::Renamed {
            from: s("Unconfirmed 42.crdownload"),
            to: s("a.zip.crdownload"),
        };
        assert!(matches!(
            d.change(named, t),
            Some(Update::Running { id: 1, ref name, .. }) if name == "a.zip"
        ));
        let done = Change::Renamed {
            from: s("a.zip.crdownload"),
            to: s("a.zip"),
        };
        assert_eq!(
            d.change(done, t),
            Some(Update::Finished {
                id: 1,
                name: s("a.zip")
            })
        );
        assert_eq!(d.change(Change::Modified(s("a.zip")), t), None);
    }

    #[test]
    fn growth_is_reported_once_a_second_and_a_running_one_is_picked_up() {
        let t = Instant::now();
        let mut d = Downloads::default();
        let grew = || Change::Modified(s("big.iso.part"));
        assert!(matches!(
            d.change(grew(), t),
            Some(Update::Running { id: 1, .. })
        ));
        assert_eq!(d.change(grew(), t + PROGRESS / 2), None);
        assert!(matches!(
            d.change(grew(), t + PROGRESS),
            Some(Update::Running { id: 1, .. })
        ));
    }

    #[test]
    fn a_deleted_partial_is_left_for_the_disk_to_decide() {
        let t = Instant::now();
        let mut d = Downloads::default();
        d.change(Change::Added(s("a.pdf.part")), t);
        assert_eq!(
            d.change(Change::Removed(s("a.pdf.part")), t),
            Some(Update::Gone {
                id: 1,
                name: s("a.pdf")
            })
        );
        assert_eq!(d.change(Change::Removed(s("a.pdf.part")), t), None);
        assert_eq!(d.change(Change::Removed(s("notes.txt")), t), None);
        assert_eq!(d.change(Change::Added(s("notes.txt")), t), None);
    }
}

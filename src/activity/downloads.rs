//! Browser downloads, told from the Downloads folder's changes (`model::browser_download`). The
//! folder's thread sizes the files itself, so the disk is never asked from the UI thread.

use std::time::Instant;

use super::Event;
use crate::log::LogErr;
use crate::model::activity::{Activity, Key};
use crate::model::browser_download::{Downloads, Update};
use crate::win32::folder;

pub fn watch(events: async_channel::Sender<Event>) {
    let Some(dir) = folder::downloads() else {
        return crate::log("no Downloads folder: not watching downloads");
    };
    let mut downloads = Downloads::default();
    let at = dir.clone();
    let len = move |name: &str| std::fs::metadata(at.join(name)).map_or(0, |m| m.len());
    let at = dir.clone();
    let done = move |id, name: String| {
        let path = at.join(&name);
        Event::Set(Key::Download(id), Activity::Downloaded { name, path })
    };
    folder::watch(dir, move |change| {
        let Some(update) = downloads.change(change, Instant::now()) else {
            return;
        };
        let event = match update {
            Update::Running { id, name, partial } => Event::Set(
                Key::Download(id),
                Activity::Downloading {
                    name,
                    bytes: len(&partial),
                },
            ),
            Update::Finished { id, name } => done(id, name),
            Update::Gone { id, name } if len(&name) > 0 => done(id, name),
            Update::Gone { id, .. } => Event::Remove(Key::Download(id)),
        };
        events.try_send(event).log_err();
    });
}

//! Keeping the island's shelf: parking dropped files, learning each one's kind and picture and
//! pruning the ones that went away, all off the UI thread, and saving the shelf as it changes.

use std::path::PathBuf;

use gpui::Context;

use crate::island::chip;
use crate::{Island, icon, log, tray};
use model::shelf;

impl Island {
    /// Files can move or vanish while parked; don't keep chips for ones that have.
    ///
    /// Looking asks the disk, which can take seconds for a share that has gone away, so it
    /// happens off the UI thread and the chips are dropped when the answer comes back.
    pub(crate) fn prune_missing(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self.shelf.items().iter().map(|i| i.path.clone()).collect();
        if paths.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let gone: Vec<PathBuf> = cx
                .background_executor()
                .spawn(async move { paths.into_iter().filter(|p| !p.exists()).collect() })
                .await;
            if gone.is_empty() {
                return;
            }
            this.update(cx, |this, cx| {
                let pruned = this.shelf.prune(|p| !gone.iter().any(|g| g == p));
                if pruned > 0 {
                    log(&format!("pruned {pruned} missing"));
                    this.shelf_changed(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn shelf_changed(&mut self, cx: &mut Context<Self>) {
        log(&format!("shelf now {}", self.shelf.len()));
        let gone: Vec<PathBuf> = (self.icons.keys())
            .filter(|path| !self.shelf.items().iter().any(|item| &item.path == *path))
            .cloned()
            .collect();
        let icons = gone
            .iter()
            .filter_map(|path| self.icons.remove(path))
            .collect();
        icon::forget(icons, cx);
        tray::set_shelf_count(self.shelf.len());
        self.fit_tray();
        if let Some(writer) = &self.writer {
            writer.send(shelf::serialize(self.shelf.items()));
        }
        cx.notify();
    }

    /// Park dropped paths on the shelf; returns how many were new.
    ///
    /// Chips go up right away: telling a folder from a file asks the disk, and a dead network
    /// share could stall the UI thread for its timeout, so the stat runs in the background
    /// (chips show `Kind::Unknown` meanwhile). Answers are merged back by path, so a chip
    /// removed or cleared before they arrive stays gone.
    pub(crate) fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) -> usize {
        let added = self.shelf.add(paths);
        self.shelf_changed(cx);
        if added.is_empty() {
            return 0;
        }
        let count = added.len();
        self.resolve_kinds(added, cx);
        count
    }

    /// Learn whether each chip is a folder or a file, and fetch its picture, off the UI thread.
    /// Each chip updates as its answer arrives.
    fn resolve_kinds(&mut self, added: Vec<PathBuf>, cx: &mut Context<Self>) {
        let rx = icon::resolve(added, (chip::ICON * self.scale).round() as i32);
        cx.spawn(async move |this, cx| {
            while let Ok((path, is_dir, icon)) = rx.recv().await {
                let updated = this.update(cx, |this, cx| {
                    if this.shelf.resolve_kinds([(path.clone(), is_dir)]) == 0 {
                        return;
                    }
                    if let Some(icon) = icon
                        && let Some(old) = this.icons.insert(path, icon)
                    {
                        icon::forget(vec![old], cx);
                    }
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// The shelf was restored from disk without looking at its files: learn their kinds and
    /// drop the ones that went away while the app was off, both off the UI thread.
    pub(crate) fn settle_restored(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self.shelf.items().iter().map(|i| i.path.clone()).collect();
        if paths.is_empty() {
            return;
        }
        self.resolve_kinds(paths, cx);
        self.prune_missing(cx);
    }
}

//! The pictures the strip's cards show (app icons, copied images, file thumbnails, site icons),
//! each loaded once, off the UI thread, as its card is first shown.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{Context, RenderImage};
use windows::Win32::System::Com::COINIT_MULTITHREADED;

use super::favicon;
use crate::{icon, win32};

/// A picture a card wants.
pub enum Want {
    /// The shell's picture of a file, about this many logical pixels square.
    Shell(PathBuf, f32),
    /// A site's icon, kept at the path.
    Site(PathBuf, String),
}

impl Want {
    fn path(&self) -> &PathBuf {
        match self {
            Want::Shell(path, _) | Want::Site(path, _) => path,
        }
    }
}

#[derive(Default)]
pub struct Pictures {
    loaded: HashMap<PathBuf, Arc<RenderImage>>,
    asked: HashSet<PathBuf>,
}

impl Pictures {
    pub fn get(&self, path: &PathBuf) -> Option<Arc<RenderImage>> {
        self.loaded.get(path).cloned()
    }

    /// Load the wanted pictures not asked for before, on a worker thread of its own (the shell
    /// wants COM there); `pictures` finds this store in the view, and each card updates as its
    /// picture arrives.
    pub fn fetch<T: 'static>(
        &mut self,
        wants: impl IntoIterator<Item = Want>,
        scale: f32,
        cx: &mut Context<T>,
        pictures: fn(&mut T) -> &mut Pictures,
    ) {
        let mut wanted: Vec<Want> = wants
            .into_iter()
            .filter(|want| self.asked.insert(want.path().clone()))
            .collect();
        if wanted.is_empty() {
            return;
        }
        // The shell's pictures are quick; the network waits behind them.
        wanted.sort_by_key(|want| matches!(want, Want::Site(..)));
        let (tx, rx) = async_channel::unbounded();
        std::thread::spawn(move || {
            win32::with_com(COINIT_MULTITHREADED, || {
                for want in wanted {
                    let picture = match &want {
                        Want::Shell(path, size) => icon::load(path, (size * scale).round() as i32),
                        Want::Site(path, host) => favicon::load(path, host),
                    };
                    if tx.send_blocking((want.path().clone(), picture)).is_err() {
                        break;
                    }
                }
            })
        });
        cx.spawn(async move |this, cx| {
            while let Ok((path, picture)) = rx.recv().await {
                let Some(picture) = picture else {
                    continue;
                };
                let updated = this.update(cx, |this, cx| {
                    pictures(this).loaded.insert(path, picture);
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }
}

//! Renaming a file in the basket, as Explorer does: a field in place of the pill, with the name
//! before the extension selected. Typing needs the keyboard, so the basket takes the focus for
//! as long as the field is up (and only then), and hands it back to the app that had it.

use std::io;
use std::path::{Path, PathBuf};

use gpui::{AsyncWindowContext, ClipboardItem, Context, KeyDownEvent, WeakEntity, Window};
use windows::Win32::Foundation::HWND;

use super::Basket;
use crate::log::LogErr;
use crate::{log, win32};
use model::name_field::NameField;
use model::shelf::{Item, Kind};
use model::transform::name_problem;

pub(super) struct Rename {
    pub path: PathBuf,
    pub field: NameField,
    /// Why the name can't be used, shown over the field.
    pub problem: Option<String>,
    /// The window that was in front, as an address, to hand the focus back to.
    previous: usize,
    /// The move is on its way to the disk.
    busy: bool,
}

impl Basket {
    /// Put the field up for `item` and take the focus. Run from a task: taking the foreground
    /// sends this window messages, which must not reach GPUI inside an update.
    pub(super) async fn begin_rename(
        this: WeakEntity<Self>,
        item: Item,
        cx: &mut AsyncWindowContext,
    ) {
        let hwnd = this.update(cx, |this, cx| {
            log(&format!("basket rename {}", item.name()));
            this.rename = Some(Rename {
                field: NameField::new(&item.name(), item.kind == Kind::Dir),
                path: item.path,
                problem: None,
                previous: win32::foreground_window().0 as usize,
                busy: false,
            });
            cx.notify();
            this.hwnd
        });
        if let Ok(Some(hwnd)) = hwnd {
            win32::bring_to_front(hwnd);
        }
        this.update_in(cx, |this, window, _| window.focus(&this.focus))
            .log_err();
    }

    /// Put the field away, and give the focus back if the basket still has it.
    pub(super) fn end_rename(&mut self, cx: &mut Context<Self>) {
        let Some(rename) = self.rename.take() else {
            return;
        };
        cx.notify();
        let Some(hwnd) = self.hwnd else {
            return;
        };
        let (raw, previous) = (hwnd.0 as usize, rename.previous);
        cx.spawn(async move |_, _| {
            if win32::foreground_window().0 as usize == raw {
                win32::restore_foreground(HWND(previous as *mut _));
            }
        })
        .detach();
    }

    /// A click elsewhere: the name as typed is kept, as in Explorer, if it can be.
    pub fn deactivated(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.rename.as_ref().is_some_and(|r| !r.busy) {
            self.commit(false, cx);
        }
    }

    pub(super) fn on_rename_key(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(rename) = self.rename.as_mut().filter(|r| !r.busy) else {
            return;
        };
        let key = &event.keystroke;
        let (ctrl, shift) = (key.modifiers.control, key.modifiers.shift);
        let field = &mut rename.field;
        match key.key.as_str() {
            "escape" => {
                cx.stop_propagation();
                return self.end_rename(cx);
            }
            "enter" => {
                cx.stop_propagation();
                return self.commit(true, cx);
            }
            "backspace" => field.backspace(),
            "delete" => field.delete(),
            "left" => field.left(shift),
            "right" => field.right(shift),
            "home" => field.home(shift),
            "end" => field.end(shift),
            "a" if ctrl => field.select_all(),
            "c" | "x" if ctrl => {
                let picked = field.text()[field.selection()].to_owned();
                if !picked.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(picked));
                    if key.key == "x" {
                        field.insert("");
                    }
                }
            }
            "v" if ctrl => {
                if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    field.insert(text.lines().next().unwrap_or_default());
                }
            }
            // GPUI names the space key and gives it no typed character.
            "space" if !ctrl && !key.modifiers.alt => field.insert(" "),
            _ => match key.key_char.as_deref() {
                Some(typed) if !ctrl && !key.modifiers.alt => field.insert(typed),
                _ => return,
            },
        }
        rename.problem = name_problem(rename.field.text()).map(str::to_owned);
        cx.stop_propagation();
        cx.notify();
    }

    /// Rename the file on disk to what was typed. On a thread: the disk can stall. `keep`
    /// leaves the field up to fix a name that can't be used; without it (a click elsewhere)
    /// such a name is dropped.
    fn commit(&mut self, keep: bool, cx: &mut Context<Self>) {
        let Some(rename) = self.rename.as_mut() else {
            return;
        };
        let name = rename.field.text().to_owned();
        let from = rename.path.clone();
        let Some(to) = from.parent().map(|dir| dir.join(&name)) else {
            return self.end_rename(cx);
        };
        if from.file_name() == to.file_name() {
            return self.end_rename(cx);
        }
        if let Some(problem) = name_problem(&name) {
            rename.problem = Some(problem.into());
            cx.notify();
            if !keep {
                self.end_rename(cx);
            }
            return;
        }
        rename.busy = true;
        let moved = cx.background_executor().spawn({
            let (from, to) = (from.clone(), to.clone());
            async move { move_to(&from, &to) }
        });
        cx.spawn(async move |this, cx| {
            let moved = moved.await;
            this.update(cx, |this, cx| {
                let still = this.rename.as_ref().is_some_and(|r| r.path == from);
                match moved {
                    Ok(()) => {
                        log(&format!("basket renamed to {name}"));
                        let renamed = [(from, to)];
                        this.follow_renames(&renamed, cx);
                        if let Some(island) = this.island.upgrade() {
                            island.update(cx, |island, cx| island.follow_renames(&renamed, cx));
                        }
                        if still {
                            this.end_rename(cx);
                        }
                    }
                    Err(err) => {
                        eprintln!("topsy: could not rename {}: {err}", from.display());
                        match this.rename.as_mut().filter(|_| still && keep) {
                            Some(rename) => {
                                rename.busy = false;
                                rename.problem = Some(why(&err).into());
                                cx.notify();
                            }
                            None if still => this.end_rename(cx),
                            None => {}
                        }
                    }
                }
            })
            .ok();
        })
        .detach();
    }
}

/// A rename that never replaces another file; a change of case alone is allowed.
fn move_to(from: &Path, to: &Path) -> io::Result<()> {
    let same = to.to_string_lossy().to_lowercase() == from.to_string_lossy().to_lowercase();
    if !same && to.exists() {
        return Err(io::ErrorKind::AlreadyExists.into());
    }
    win32::move_new(from, to)
}

fn why(err: &io::Error) -> &'static str {
    match err.kind() {
        io::ErrorKind::AlreadyExists => "There is already a file with that name",
        io::ErrorKind::PermissionDenied => "Windows won't allow that rename",
        io::ErrorKind::NotFound => "The file isn't there any more",
        _ => "That rename didn't work",
    }
}

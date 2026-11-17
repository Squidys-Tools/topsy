//! OLE drag and drop: watching drags onto the island and dragging shelf files out.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{HWND, POINTL};
use windows::Win32::System::Com::{CoTaskMemFree, IDataObject};
use windows::Win32::System::Ole::{
    DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE, IDropTarget,
    IDropTarget_Impl, RegisterDragDrop, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    BHID_DataObject, SHCreateShellItemArrayFromIDLists, SHDoDragDrop, SHParseDisplayName,
};
use windows::Win32::UI::WindowsAndMessaging::GetPropW;
use windows::core::{Interface, PCWSTR, Ref, implement, w};

use super::wide;

/// The `IDropTarget` registered on the window. `RegisterDragDrop` records it in this
/// window property; GPUI keeps no reference of its own.
fn registered_drop_target(hwnd: HWND) -> Option<IDropTarget> {
    let raw = unsafe { GetPropW(hwnd, w!("OleDropTargetInterface")) };
    unsafe { IDropTarget::from_raw_borrowed(&raw.0) }.cloned()
}

/// What a drag out of the shelf ended as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragOutcome {
    /// Released over a target that took the files (copy, move or link).
    Dropped,
    /// Escape, or released over something that refuses files.
    Cancelled,
}

/// Takes a window out of OLE drag-and-drop while one of its own items is being dragged.
///
/// GPUI registers every window as a drop target for files. During a drag out of the shelf
/// the cursor starts over that window, so the system's drag loop would offer the drag to
/// the window it came from, and GPUI's handler would read the dragged files straight back
/// out of the shell's data object. That intermittently blocked the UI thread for seconds
/// while the shell loaded handlers, with the drag loop never returning. Nothing is lost by
/// pausing it: dropping a chip back on the shelf is not an operation.
///
/// GPUI keeps no reference to its drop target (the registration owns it), so it is read
/// back from the property OLE stores it in, kept alive across the revoke, and registered
/// again on drop. If the property is missing the window is left registered.
struct PausedDropTarget {
    hwnd: HWND,
    target: Option<IDropTarget>,
}

impl PausedDropTarget {
    fn new(hwnd: HWND) -> Self {
        let target = registered_drop_target(hwnd);
        match &target {
            Some(_) => {
                let _ = unsafe { RevokeDragDrop(hwnd) };
            }
            None => eprintln!(
                "topsy: the island's drop target was not found, so a drag out may stall (is OleDropTargetInterface gone?)"
            ),
        }
        Self { hwnd, target }
    }
}

impl Drop for PausedDropTarget {
    fn drop(&mut self) {
        if let Some(target) = &self.target
            && let Err(err) = unsafe { RegisterDragDrop(self.hwnd, target) }
        {
            eprintln!("topsy: could not restore the island as a drop target: {err}");
        }
    }
}

/// What the island's drop target reports about a drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropEvent {
    /// The drag crossing the island was refused: the payload can't be dropped here
    /// (the hook that fired the jiggle can't tell a text selection from files).
    Refused,
}

/// A drop target that forwards everything to GPUI's and reports refusals.
///
/// The jiggle hook can't see what a drag carries, so a jiggle during a text drag
/// opens the tray for nothing. GPUI's own target refuses such drags silently —
/// `DROPEFFECT_NONE`, no event — so the island swaps its registered target for this
/// one and learns the refusal over a channel, the same way the hook and tray icon
/// report in.
#[implement(IDropTarget)]
struct WatchingDropTarget {
    inner: IDropTarget,
    tx: async_channel::Sender<DropEvent>,
    /// Did the current drag get an effect at `DragEnter`? Calls are serial on the
    /// registering thread.
    accepted: AtomicBool,
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for WatchingDropTarget_Impl {
    fn DragEnter(
        &self,
        pdataobj: Ref<IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        self.accepted.store(false, Ordering::Relaxed);
        let result = unsafe {
            self.inner
                .DragEnter(pdataobj.ok()?, grfkeystate, *pt, pdweffect)
        };
        let refused = result.is_ok() && unsafe { *pdweffect } == DROPEFFECT_NONE;
        self.accepted
            .store(result.is_ok() && !refused, Ordering::Relaxed);
        if refused {
            let _ = self.tx.try_send(DropEvent::Refused);
        }
        result
    }

    fn DragOver(
        &self,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let result = unsafe { self.inner.DragOver(grfkeystate, *pt, pdweffect) };
        if !self.accepted.load(Ordering::Relaxed) {
            // GPUI answers COPY unconditionally; a payload refused at entry must keep
            // the "no" answer, or it shows a copy cursor and takes a drop it can't fulfil.
            unsafe { *pdweffect = DROPEFFECT_NONE };
        }
        result
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        self.accepted.store(false, Ordering::Relaxed);
        unsafe { self.inner.DragLeave() }
    }

    fn Drop(
        &self,
        pdataobj: Ref<IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        self.accepted.store(false, Ordering::Relaxed);
        unsafe { self.inner.Drop(pdataobj.ok()?, grfkeystate, *pt, pdweffect) }
    }
}

/// Swap a window's registered drop target for a [`WatchingDropTarget`], so a refused
/// drag — the payload a jiggle opened the basket for turning out to be undroppable —
/// reaches the app on `tx`. Call once, on the thread the window belongs to, after GPUI
/// has registered its own target. Without one to wrap, the window is left as it was.
pub fn watch_drop_refusals(hwnd: HWND, tx: async_channel::Sender<DropEvent>) {
    let Some(inner) = registered_drop_target(hwnd) else {
        eprintln!("topsy: no drop target to watch (is OleDropTargetInterface gone?)");
        return;
    };
    let _ = unsafe { RevokeDragDrop(hwnd) };
    let watcher: IDropTarget = WatchingDropTarget {
        inner: inner.clone(),
        tx,
        accepted: AtomicBool::new(false),
    }
    .into();
    if let Err(err) = unsafe { RegisterDragDrop(hwnd, &watcher) } {
        eprintln!("topsy: could not watch the drop target: {err}");
        // Leave the island as plain a drop target as before: put GPUI's back.
        let _ = unsafe { RegisterDragDrop(hwnd, &inner) };
    }
}

/// Drag `paths` out to whatever window the cursor ends up over, as Explorer would for the
/// same selection. Blocks in the system's drag loop until the button is released, so it
/// must be called with the left button down, and from a task, never from inside an event
/// handler (the loop dispatches this window's messages back into GPUI).
///
/// The data object comes from the shell (`BHID_DataObject` on the items), so it carries
/// everything real targets expect, and `SHDoDragDrop` with no drop source supplies the
/// shell's own: the drag image, the cursors, Escape to cancel. No COM is implemented here.
pub fn drag_files_out(hwnd: HWND, paths: &[PathBuf]) -> windows::core::Result<DragOutcome> {
    unsafe {
        let mut pidls: Vec<*const ITEMIDLIST> = Vec::with_capacity(paths.len());
        let free_all = |pidls: &[*const ITEMIDLIST]| {
            for pidl in pidls {
                CoTaskMemFree(Some(pidl.cast()));
            }
        };
        for path in paths {
            let wide = wide(path);
            let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
            if let Err(err) = SHParseDisplayName(PCWSTR(wide.as_ptr()), None, &mut pidl, 0, None) {
                free_all(&pidls);
                return Err(err);
            }
            pidls.push(pidl);
        }
        let array = SHCreateShellItemArrayFromIDLists(&pidls);
        free_all(&pidls);
        let data: IDataObject = array?.BindToHandler(None, &BHID_DataObject)?;

        let _own_target_paused = PausedDropTarget::new(hwnd);
        let effect = SHDoDragDrop(
            Some(hwnd),
            &data,
            None,
            DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK,
        )?;
        Ok(if effect == DROPEFFECT_NONE {
            DragOutcome::Cancelled
        } else {
            DragOutcome::Dropped
        })
    }
}

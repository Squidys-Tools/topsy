//! The Win32 glue GPUI doesn't expose: overlay window styles, glass, window geometry,
//! and the global mouse hook that watches for the jiggle gesture.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, POINTL, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, GetMonitorInfoW,
    MONITOR_DEFAULTTONULL, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow, PtInRect,
    RGN_OR, SetWindowRgn,
};
use windows::Win32::System::Com::{
    COINIT, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, IDataObject,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Ole::{
    DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE, IDropTarget,
    IDropTarget_Impl, RegisterDragDrop, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    BHID_DataObject, SHCreateShellItemArrayFromIDLists, SHDoDragDrop, SHParseDisplayName,
    ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetCursorPos,
    GetForegroundWindow, GetMessageW, GetPropW, GetShellWindow, GetWindowLongPtrW, GetWindowRect,
    HHOOK, HWND_TOPMOST, IsZoomed, MSG, MSLLHOOKSTRUCT, SW_SHOWNORMAL, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetWindowLongPtrW,
    SetWindowPos, SetWindowsHookExW, TranslateMessage, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
};
use windows::core::{Interface, PCWSTR, Ref, implement, s, w};

use crate::edge::{EdgeWatch, EdgeZone, fills_monitor};
use crate::jiggle::{JiggleConfig, JiggleDetector};

/// Whether the foreground window is a fullscreen app on `island`'s monitor. The desktop
/// counts as neither: it fills the monitor too, but its windows are `Progman` and `WorkerW`.
pub fn fullscreen_over(island: HWND) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg == island || fg == GetShellWindow() {
            return false;
        }
        let mut class = [0u16; 16];
        let len = GetClassNameW(fg, &mut class) as usize;
        if matches!(
            String::from_utf16_lossy(&class[..len]).as_str(),
            "Progman" | "WorkerW"
        ) {
            return false;
        }
        let monitor = MonitorFromWindow(island, MONITOR_DEFAULTTOPRIMARY);
        if MonitorFromWindow(fg, MONITOR_DEFAULTTONULL) != monitor {
            return false;
        }
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let mut rect = RECT::default();
        if !GetMonitorInfoW(monitor, &mut info).as_bool() || GetWindowRect(fg, &mut rect).is_err() {
            return false;
        }
        let m = info.rcMonitor;
        fills_monitor(
            (rect.left, rect.top, rect.right, rect.bottom),
            (m.left, m.top, m.right, m.bottom),
            IsZoomed(fg).as_bool(),
        )
    }
}

pub fn hwnd_from_raw(handle: raw_window_handle::RawWindowHandle) -> Option<HWND> {
    match handle {
        raw_window_handle::RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut c_void)),
        _ => None,
    }
}

/// Make the window an overlay: topmost, never takes focus from the app being used.
///
/// GPUI ignores `WindowOptions::focus` on Windows and activates the window when it is
/// created. A window created with `show: false` is not an option: GPUI defers its
/// placement until `activate()` and never paints it. So the window is created visible,
/// this applies `WS_EX_NOACTIVATE` so nothing activates it afterwards, and the caller
/// hands foreground back to whatever had it (see [`foreground_window`]).
///
/// It also makes the window a bare `WS_POPUP`. GPUI creates popups with style 0, which
/// Windows treats as an overlapped window: it enforces a minimum height and, once GPUI is
/// told not to hide the titlebar, reserves a caption. A bare popup has no non-client area,
/// so the client area is the whole window, which the pill-sized acrylic window relies on.
pub fn apply_overlay_styles(hwnd: HWND) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let ex = ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
        SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_POPUP.0 | WS_VISIBLE.0) as isize);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

#[repr(C)]
struct AccentPolicy {
    state: u32,
    flags: u32,
    /// 0xAABBGGRR
    gradient_color: u32,
    animation_id: u32,
}

#[repr(C)]
struct CompositionAttribData {
    attrib: u32,
    data: *mut c_void,
    size: usize,
}

const WCA_ACCENT_POLICY: u32 = 0x13;
const ACCENT_DISABLED: u32 = 0;
const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;

/// Turn on the system acrylic blur behind the window, tinted with `tint` (0xRRGGBBAA).
///
/// Uses the undocumented `SetWindowCompositionAttribute`, which is how GPUI's own
/// `WindowBackgroundAppearance::Blurred` works on Windows. It is called directly here
/// only to choose the tint, since GPUI's version hardcodes a near-clear one.
///
/// The blur fills the window's whole rectangle and, on current Windows 11, ignores
/// `SetWindowRgn`, so the window has to *be* the shape that should be blurred
/// (see `Island::sync_window`) and DWM rounds its corners ([`round_corners`]).
pub fn set_acrylic(hwnd: HWND, tint: Option<u32>) {
    type SetWindowCompositionAttribute =
        unsafe extern "system" fn(HWND, *mut CompositionAttribData) -> i32;
    unsafe {
        let Ok(user32) = GetModuleHandleW(w!("user32.dll")) else {
            return;
        };
        let Some(proc) = GetProcAddress(user32, s!("SetWindowCompositionAttribute")) else {
            return;
        };
        let set_attr: SetWindowCompositionAttribute = std::mem::transmute(proc);
        let [r, g, b, a] = tint.unwrap_or(0).to_be_bytes();
        let mut accent = AccentPolicy {
            state: if tint.is_some() {
                ACCENT_ENABLE_ACRYLICBLURBEHIND
            } else {
                ACCENT_DISABLED
            },
            flags: 0,
            gradient_color: u32::from_le_bytes([r, g, b, a]),
            animation_id: 0,
        };
        let mut data = CompositionAttribData {
            attrib: WCA_ACCENT_POLICY,
            data: &mut accent as *mut _ as *mut c_void,
            size: std::mem::size_of::<AccentPolicy>(),
        };
        if set_attr(hwnd, &mut data) == 0 {
            eprintln!("topsy: SetWindowCompositionAttribute failed; no glass blur");
        }
    }
}

/// Ask DWM to round the window's corners (and so its acrylic backdrop) by the system radius.
pub fn round_corners(hwnd: HWND) {
    let preference = DWMWCP_ROUND;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const c_void,
            std::mem::size_of_val(&preference) as u32,
        );
    }
}

/// Horizontal centre and top edge, in physical screen pixels, of the monitor the window is on.
pub fn monitor_top_center(hwnd: HWND) -> Option<(i32, i32)> {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
    }
    let r = info.rcMonitor;
    Some(((r.left + r.right) / 2, r.top))
}

/// Move and resize without activating.
///
/// Not for use from inside GPUI's `render`: this is synchronous (`SWP_ASYNCWINDOWPOS` only
/// defers across threads), so `WM_SIZE` would reach GPUI mid-draw, where its resize
/// callback cannot update the window, and GPUI would never learn the new size. Run it
/// from a foreground task instead, as GPUI's own `Window::resize` does.
///
/// `raise` also puts it above every other topmost window: the one given foreground back at
/// launch, or any that came along while the notch was hidden, would otherwise cover it.
pub fn move_window(hwnd: HWND, x: i32, y: i32, w: i32, h: i32, raise: bool) {
    let (after, order) = if raise {
        (Some(HWND_TOPMOST), Default::default())
    } else {
        (None, SWP_NOZORDER)
    };
    unsafe {
        let _ = SetWindowPos(hwnd, after, x, y, w, h, SWP_NOACTIVATE | order);
    }
}

/// What of the window may draw and take clicks, in physical pixels of the window.
pub enum Clip {
    /// Nothing: the notch is hidden.
    Nothing,
    /// The whole window (the acrylic fallback, which DWM rounds itself).
    Everything,
    /// A notch body with square top, rounded bottom and an ear strip along the top edge.
    Notch {
        width: i32,
        height: i32,
        ear_slot: i32,
        ear: i32,
        radius: i32,
    },
}

/// Shape the window with a region, so the transparent corners outside the notch are
/// click-through.
///
/// The region clips drawing too, without anti-aliasing, so its corners are a pixel looser than
/// the drawn ones: it then falls just outside the compositor's smooth edge instead of cutting it.
/// The ears get their whole strip: what lies outside their curve is a few pixels.
pub fn set_clip(hwnd: HWND, clip: Clip) {
    unsafe {
        let region = match clip {
            Clip::Everything => None,
            Clip::Nothing => Some(CreateRectRgn(0, 0, 0, 0)),
            Clip::Notch {
                width,
                height,
                ear_slot,
                ear,
                radius,
            } => {
                let r = (radius - 1).max(0);
                // Starting the round rectangle above the window squares off its top corners.
                // The ellipse is given by its diameter, and the right/bottom edges are exclusive.
                let body = CreateRoundRectRgn(
                    ear_slot,
                    -2 * r,
                    width - ear_slot + 1,
                    height + 1,
                    2 * r,
                    2 * r,
                );
                let ears = CreateRectRgn(ear_slot - ear, 0, width - ear_slot + ear, ear);
                CombineRgn(Some(body), Some(body), Some(ears), RGN_OR);
                let _ = DeleteObject(ears.into());
                Some(body)
            }
        };
        // The system owns the region from here on.
        SetWindowRgn(hwnd, region, false);
    }
}

/// `s` as the NUL-terminated UTF-16 the Windows APIs take.
pub fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

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

/// Swap the island's registered drop target for a [`WatchingDropTarget`], so a refused
/// drag — the payload a jiggle opened the tray for turning out to be undroppable —
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

/// Run `f` with COM set up on this thread, which a fresh thread doesn't have and the shell's
/// APIs need. A single-threaded apartment only answers calls from other apartments while it
/// pumps messages, so a thread that doesn't pump and calls out-of-process handlers (thumbnails)
/// wants `COINIT_MULTITHREADED`.
pub fn with_com<T>(apartment: COINIT, f: impl FnOnce() -> T) -> T {
    let com = unsafe { CoInitializeEx(None, apartment | COINIT_DISABLE_OLE1DDE) };
    let result = f();
    if com.is_ok() {
        unsafe { CoUninitialize() };
    }
    result
}

/// Open a file or folder with its default handler, as a double-click would.
///
/// Blocks until the handler has been started, which can be a while (a UAC prompt, a cold
/// network path), and pumps messages while it waits. Call it from a thread of its own, never
/// the UI thread: it would freeze the island, and a message for the island's window reaching
/// GPUI from inside one of its event handlers aborts the process.
pub fn open_path(path: &Path) -> bool {
    let wide = wide(path);
    // ShellExecute wants COM on the calling thread.
    let result = with_com(COINIT_APARTMENTTHREADED, || unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    });
    // ShellExecute reports success as a value above 32.
    result.0 as usize > 32
}

/// Is the cursor over the window's rectangle right now? Asked of the system directly
/// rather than inferred from hover events, which can miss a "leave" while a drag has the mouse.
pub fn cursor_over(hwnd: HWND) -> bool {
    let mut at = POINT::default();
    let mut rect = RECT::default();
    unsafe {
        GetCursorPos(&mut at).is_ok()
            && GetWindowRect(hwnd, &mut rect).is_ok()
            && PtInRect(&rect, at).as_bool()
    }
}

pub fn foreground_window() -> HWND {
    unsafe { GetForegroundWindow() }
}

/// Give foreground back to `previous` (the window that had it before ours opened).
pub fn restore_foreground(previous: HWND) {
    if !previous.0.is_null() {
        unsafe {
            let _ = SetForegroundWindow(previous);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEvent {
    Jiggle,
    ButtonUp,
    /// The cursor arrived at (or left) the top edge above the notch; `pressed` while dragging.
    Edge {
        at_edge: bool,
        pressed: bool,
    },
}

struct HookState {
    detector: JiggleDetector,
    edge: Option<EdgeWatch>,
    tx: async_channel::Sender<HookEvent>,
}

static HOOK_STATE: Mutex<Option<HookState>> = Mutex::new(None);
static STARTED: OnceLock<()> = OnceLock::new();

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let now = Instant::now();
        if let Ok(mut guard) = HOOK_STATE.try_lock()
            && let Some(state) = guard.as_mut()
        {
            match wparam.0 as u32 {
                WM_LBUTTONDOWN => state.detector.button_down(now, info.pt.x),
                WM_LBUTTONUP => {
                    state.detector.button_up();
                    let _ = state.tx.try_send(HookEvent::ButtonUp);
                }
                WM_MOUSEMOVE => {
                    if let Some(at_edge) = state
                        .edge
                        .as_mut()
                        .and_then(|e| e.moved(info.pt.x, info.pt.y))
                    {
                        let pressed = state.detector.pressed();
                        let _ = state.tx.try_send(HookEvent::Edge { at_edge, pressed });
                    }
                    if state.detector.moved(now, info.pt.x) {
                        let _ = state.tx.try_send(HookEvent::Jiggle);
                    }
                }
                _ => {}
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Install the low-level mouse hook on a dedicated thread (hooks need a message
/// loop on the thread that installed them). Safe to call once; later calls are no-ops.
pub fn spawn_mouse_hook(
    config: JiggleConfig,
    edge: Option<EdgeZone>,
) -> async_channel::Receiver<HookEvent> {
    let (tx, rx) = async_channel::bounded(8);
    if STARTED.set(()).is_err() {
        return rx;
    }
    *HOOK_STATE.lock().unwrap() = Some(HookState {
        detector: JiggleDetector::new(config),
        edge: edge.map(EdgeWatch::new),
        tx,
    });
    std::thread::Builder::new()
        .name("topsy-mouse-hook".into())
        .spawn(|| unsafe {
            let hook: HHOOK = match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0) {
                Ok(h) => h,
                Err(err) => {
                    eprintln!("topsy: could not install mouse hook: {err}");
                    return;
                }
            };
            let _keep_alive = hook;
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("spawn mouse hook thread");
    rx
}

//! The Win32 glue GPUI doesn't expose: overlay window styles, glass, window geometry,
//! and the global mouse hook that watches for the jiggle gesture.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow, PtInRect,
};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, IDataObject,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Ole::{
    DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE, IDropTarget,
    RegisterDragDrop, RevokeDragDrop,
};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    BHID_DataObject, SHCreateShellItemArrayFromIDLists, SHDoDragDrop, SHParseDisplayName,
    ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GWL_EXSTYLE, GWL_STYLE, GetCursorPos, GetForegroundWindow,
    GetMessageW, GetPropW, GetWindowLongPtrW, GetWindowRect, HHOOK, HWND_TOPMOST, MSG,
    MSLLHOOKSTRUCT, SW_SHOWNORMAL, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowsHookExW,
    TranslateMessage, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
};
use windows::core::{Interface, PCWSTR, s, w};

use crate::jiggle::{JiggleConfig, JiggleDetector};

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
pub fn enable_acrylic(hwnd: HWND, tint: u32) {
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
        let [r, g, b, a] = tint.to_be_bytes();
        let mut accent = AccentPolicy {
            state: ACCENT_ENABLE_ACRYLICBLURBEHIND,
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
pub fn move_window(hwnd: HWND, x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOACTIVATE | SWP_NOZORDER);
    }
}

/// `s` as the NUL-terminated UTF-16 the Windows APIs take.
pub fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
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
        // `RegisterDragDrop` records the target in this window property.
        let raw = unsafe { GetPropW(hwnd, w!("OleDropTargetInterface")) };
        let target = unsafe { IDropTarget::from_raw_borrowed(&raw.0) }.cloned();
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

/// Open a file or folder with its default handler, as a double-click would.
///
/// Blocks until the handler has been started, which can be a while (a UAC prompt, a cold
/// network path), and pumps messages while it waits. Call it from a thread of its own, never
/// the UI thread: it would freeze the island, and a message for the island's window reaching
/// GPUI from inside one of its event handlers aborts the process.
pub fn open_path(path: &Path) -> bool {
    let wide = wide(path);
    // ShellExecute wants COM on the calling thread, and a fresh thread has none.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if com.is_ok() {
        unsafe { CoUninitialize() };
    }
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
}

struct HookState {
    detector: JiggleDetector,
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
                WM_MOUSEMOVE if state.detector.moved(now, info.pt.x) => {
                    let _ = state.tx.try_send(HookEvent::Jiggle);
                }
                _ => {}
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Install the low-level mouse hook on a dedicated thread (hooks need a message
/// loop on the thread that installed them). Safe to call once; later calls are no-ops.
pub fn spawn_mouse_hook(config: JiggleConfig) -> async_channel::Receiver<HookEvent> {
    let (tx, rx) = async_channel::bounded(8);
    if STARTED.set(()).is_err() {
        return rx;
    }
    *HOOK_STATE.lock().unwrap() = Some(HookState {
        detector: JiggleDetector::new(config),
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

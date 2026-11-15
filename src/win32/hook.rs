//! The global low-level mouse hook that watches for the jiggle gesture and the top edge.

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HHOOK, MSG, MSLLHOOKSTRUCT, SetWindowsHookExW,
    TranslateMessage, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
};

use crate::edge::{EdgeWatch, EdgeZone};
use crate::jiggle::{JiggleConfig, JiggleDetector};

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

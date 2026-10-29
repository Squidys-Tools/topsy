# topsy

A Droppy-style notch/island for Windows, written in Rust on [GPUI](https://crates.io/crates/gpui).

It started as a **feasibility spike** for the riskiest parts of the design, and all four held up:

1. A transparent, always-on-top, non-activating glass pill window with spring animation.
2. A global mouse hook that detects the "jiggle a drag" gesture.
3. Receiving a real file drag on that window.
4. A real blur behind the pill, in the pill's shape.

On top of that it is now a usable v1: the tray holds a **shelf** of files you can drag back out, there is a
**tray icon** with a menu, **start with Windows**, and a **Quit** item. See
[Shelf, tray icon and startup](#shelf-tray-icon-and-startup).

## What's here

| File | Purpose |
|---|---|
| `src/spring.rs` | Damped spring (Framer-Motion-style `stiffness/damping/mass`), fixed sub-step, retargetable mid-flight |
| `src/jiggle.rs` | Pure jiggle detector: direction reversals of a held-button drag inside a time window |
| `src/win32.rs` | Overlay window styles, acrylic glass, window geometry, `WH_MOUSE_LL` hook thread, the drop-target wrapper that reports refused drags, dragging files out, opening them |
| `src/shelf.rs` | The shelf model: files parked on the island (no UI, no OS calls, unit tested) |
| `src/state.rs` | The shelf's save file and the thread that writes it |
| `src/tray_cause.rs` | Why the tray is open, and which events fold it early (pure, unit tested) |
| `src/tray.rs` | The notification-area icon and its menu, on their own thread |
| `src/autostart.rs` | Start with Windows: one `HKCU\...\Run` value |
| `src/main.rs` | The island view: idle pill, expandable tray with the shelf, file-drop target |
| `build.rs` | Copies one import library the GNU toolchain's bundled MinGW lacks (see below) |
| `rust-toolchain.toml` | Pins the GNU toolchain, `stable-x86_64-pc-windows-gnu` |

Right-click the pill or the tray icon for the menu (Quit is in it). Env vars for testing: `TOPSY_DEMO=1`
(auto expand/collapse), `TOPSY_LOG=1` (event trace on stderr), `TOPSY_EXIT_AFTER_MS=<n>`, `TOPSY_NO_HOOK=1` (don't
install the mouse hook), `TOPSY_NO_TRAY=1` (no tray icon and no menu, so right-clicking the pill quits; start-with-Windows is left
alone),
`TOPSY_SEED=<path;path>` (start with these files on the shelf), `TOPSY_STATE=<path>` (the shelf's save file,
`%LOCALAPPDATA%\topsy\shelf.txt` by default; set empty to not save), `TOPSY_GLASS=tint|solid|clear` (see
[Glass](#glass); the default is the real blur).

## Running

```powershell
cargo run            # then hold the left mouse button on a file and jiggle it side to side
cargo test           # spring, jiggle, shelf and its save file, tray icon and start-with-Windows unit tests
scripts/check.ps1    # fmt, clippy and tests, the same gates as CI; run it before pushing
```

This builds with the **GNU toolchain** (`x86_64-pc-windows-gnu`), pinned in `rust-toolchain.toml`. No MSVC.
You need:

- A MinGW-w64 `gcc` on `PATH` (e.g. `scoop install gcc`, or MSYS2). `build.rs` asks it where `libktmw32.a`
  lives: rustc's bundled MinGW ships only some of the Windows import libraries and a GPUI dependency links
  `ktmw32`. Only that one file is copied, so the build doesn't mix two MinGW runtimes. Pointing rustc's
  linker at the system gcc instead does not work: scoop's gcc has no `libgcc_eh.a`.
- The Windows SDK, for `fxc.exe` (GPUI compiles its HLSL shaders with it; set `GPUI_FXC_PATH` if it isn't found).

Use a release build to judge anything about focus or footprint. A debug build is a console app, and the
console window it opens takes foreground focus at launch. Release builds use the GUI subsystem.

### No toolchain, or no admin rights?

CI (`.github/workflows/build.yml`) builds the same release exe on a GitHub Windows runner, which has the Windows
SDK and a MinGW gcc, and uploads it. Open the run for a commit under the repository's **Actions** tab and download
the `topsy-windows-gnu` artifact: no Rust, MinGW or SDK needed to run it. A cold build takes about 20 minutes.

To build locally without installing the SDK, GPUI only needs one file from it: copy `fxc.exe` (from
`Windows Kits\10\bin\<version>\x64`) anywhere and set `GPUI_FXC_PATH` to it.

## Shelf, tray icon and startup

- **Shelf.** Drop files on the pill and they are parked in the tray as chips (an extension badge, the name, and a
  cross to remove it from the shelf; the file itself is never touched). The chips are references, not copies.
  Drag a chip out and drop it on any window, as you would from Explorer; click a chip to open it; **Clear**
  empties the shelf. A chip leaves the shelf once a drop is accepted. Files that were moved or deleted since they
  were parked are dropped from the shelf when the tray opens (checked off the UI thread). The shelf is saved to
  `%LOCALAPPDATA%\topsy\shelf.txt` on every change and comes back on the next start. Startup never looks at the saved files (a dead share would
  stall launch): chips return as `…` and are settled off the UI thread, which drops entries whose files are gone.
- **Tray icon.** A small glass-blue pill in the notification area (drawn in code, no asset). Left click opens
  or closes the shelf. Right click, or right-click the pill itself, opens the menu: *Show shelf*, *Clear shelf (N)*,
  *Start with Windows* (checked when on) and *Quit topsy*. The icon lives on its own thread with a hidden window,
  re-adds itself when Explorer restarts, and is removed on Quit. On Windows 11 a new icon starts in the overflow
  flyout; drag it out to keep it visible.
- **Start with Windows** writes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\topsy` (the quoted path of
  the running exe): per-user, no admin, nothing to uninstall. It is off until you turn it on from the menu. If
  the exe has moved since, the stored path is repaired at launch; an entry is never created by itself.

Two things about dragging out of a window that is itself a drop target, both found the hard way:

- **The island steps out of drag-and-drop while one of its chips is being dragged.** GPUI registers every window as
  a file drop target, and the drag starts over that window, so the system's drag loop offered the drag to the
  window it came from. GPUI's handler then read the files out of the shell's data object, and that
  intermittently blocked the UI thread for seconds with the loop never returning. `win32::PausedDropTarget` takes
  the registration out for the duration of the drag and puts it back afterwards (GPUI keeps no reference to its
  drop target, so it is read back from the `OleDropTargetInterface` window property and kept alive across the
  revoke). The cost is that dropping a chip back on the shelf is a cancelled drag, which is what you want anyway.
- **Anything that pumps messages must stay out of event handlers.** The drag loop does, and so does
  `ShellExecute` (opening a chip): a message for the window reaching GPUI while it is inside a click handler finds
  the app already borrowed and aborts the process. That crashed the app about one run in five in the e2e. The
  drag loop runs from a `cx.spawn` task; opening a chip runs on a thread of its own, which also keeps a slow
  handler (a UAC prompt, a cold network path) from freezing the island.

## Glass

The pill is real acrylic blur of whatever is behind it, in the pill's shape, animated.

`SetWindowCompositionAttribute` with `ACCENT_ENABLE_ACRYLICBLURBEHIND` (the call GPUI's own
`WindowBackgroundAppearance::Blurred` makes, but with our own tint) blurs the **whole window rectangle** and,
on the Windows 11 build tested (26300), ignores `SetWindowRgn`. So there is no way to clip the blur to a pill
inside a larger transparent window; a padded window shows up as a blurred box around the pill. The window
therefore **is** the pill: it is resized and re-centred every frame as the spring moves, and DWM rounds its
corners (`DWMWCP_ROUND`), which clips the acrylic with them.

Consequences and the GPUI details that had to be got right:

- **The corner radius is DWM's fixed ~8 px**, so this is a rounded rectangle, not a capsule. The drawn pill
  uses the same radius so its rim lines up with the blur.
- **The resize must not happen inside `render`.** `SWP_ASYNCWINDOWPOS` only defers across threads; here it is
  synchronous, so `WM_SIZE` reaches GPUI mid-draw, where its resize callback can't update the window and GPUI
  never learns the new size. The move runs in a foreground task (what GPUI's own `Window::resize` does). The
  pill is then drawn at the viewport size GPUI reports, so its edge always matches the window's, one frame
  behind the spring.
- **GPUI creates popups with window style 0**, which Windows treats as overlapped: it enforces a minimum height
  (39 px, taller than the idle pill) and adds a caption once the titlebar isn't "transparent". With
  `titlebar: None`, GPUI instead insets the client area by an invisible 8 px resize frame. So: pass
  `appears_transparent: false` and replace the style with a bare `WS_POPUP` right after creation.
- Because the window is the pill, nothing around it needs to be click-through any more, and `SetWindowRgn` is
  gone.

`scripts/check-glass.ps1` proves the blur rather than eyeballing it. It puts 6 px black/white stripes behind
the pill and compares stripe contrast through the pill with the contrast beside it, in three modes:

| `TOPSY_GLASS` | What it is | Contrast kept |
|---|---|---|
| `clear` | transparent pill, no blur | 1.00 (control: proves the measurement) |
| `tint` | the same colour as a plain fill, no blur | 0.45 (what the dark fill alone does) |
| *(default)* | acrylic | **0.02** (4% of what the tint leaves) |

The `tint` control matters: the fill is ~55% opaque, so it cuts stripe contrast by itself, and without the
control that would look like blur. A first attempt used `DwmEnableBlurBehindWindow`, which does honour an
explicit blur region but is a weak blur: 0.30 against the tint's 0.45, with the stripes plainly visible
through the pill. It was dropped.

## Live-desktop checks (`scripts/`)

These drive the real desktop, so they move your cursor and/or show a window for a few seconds.

- `e2e-drag.ps1` — starts a genuine OLE file drag from a fixture window, jiggles it with injected input,
  verifies the tray expands, carries the drag onto the pill, and checks the drop arrives with the right file name.
  It checks that the window under the drop point is the island's first and aborts otherwise, so a drag is never
  carried to the top of the screen unless the pill is there to catch it.
- `e2e-shelf.ps1` — seeds a shelf, clicks the pill open, drags a chip onto a separate app (a fixture window,
  `drop-target.ps1`) and checks it received the file and the chip left, cancels a drag with Escape (the chip stays),
  removes a chip with its cross without opening it, opens a folder chip in Explorer, clears the shelf, and finally
  drops a file from another app onto the island to prove it is still a drop target after being dragged out of.
  Escape is held for 250 ms, as the drag loop polls the key.
- `e2e-tray.ps1` — checks the real notification area: the icon is registered with the shell and drawn there, left
  click toggles the shelf, the menu's items work (real keystrokes), *Start with Windows* writes and then clears the
  real Run value, right-clicking the pill opens the menu, and Quit exits cleanly and removes the icon. It handles an
  auto-hidden taskbar, and briefly sets the shell's "show this icon" flag for topsy under
  `HKCU\Control Panel\NotifyIconSettings`; that flag and the Run value are restored when it finishes.
- `check-overlay.ps1` — only the pill is hit-testable, and launching doesn't change the foreground window.
  Run it on a release build (see above).
- `check-glass.ps1` — the blur proof described under [Glass](#glass). Shows a stripe window at the top of the
  screen for about 30 s.
- `trace-width.ps1` / `shot.ps1` — sample or screenshot the pill to look at the animation as composited
  (`trace-width.ps1` needs `TOPSY_GLASS=solid`).
- `measure-idle.ps1` / `count-renders.ps1` — idle footprint and re-render count.

## Findings

- **GPUI already registers an `IDropTarget` per window** and the view sees file drops through
  `on_drop::<ExternalPaths>` / `drag_over::<ExternalPaths>`, but a refused drag (anything without
  `CF_HDROP`, like a text selection) gets `DROPEFFECT_NONE` and *no* event, so the app can't hear it
  through GPUI. `win32::watch_drop_refusals` swaps the registered target for one that forwards to
  GPUI's and reports a refusal over a channel: that is the signal a jiggle-opened tray uses to learn
  that the drag it opened for — the hook can't tell text from files — can never land, and fold itself
  away. The wrapper also re-answers `DragOver` as `DROPEFFECT_NONE` for a refused payload: GPUI says
  `COPY` unconditionally, which would flash a copy cursor and let the drop through.
- **`WindowOptions::focus` is ignored on Windows**; GPUI activates the window when it is created.
  `show: false` is *not* a workaround in gpui 0.2.2: it defers placement until `activate()` and the window is
  never painted. What works: create visible, apply `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` + topmost right away,
  and `SetForegroundWindow` back to whatever had focus. After that nothing activates the island, including
  expanding the tray mid-drag (checked in the e2e).
- **`SetCursorPos` does not reliably reach `WH_MOUSE_LL`**; test input must be `mouse_event`/`SendInput`.
- Low-level hooks do see button-up and moves during an OLE drag loop.
- **A drag out with Escape needs the key held a moment.** The system's drag loop polls the key state rather than
  being sent the keystroke, because the island never takes focus. A tap shorter than a frame can be missed;
  releasing the button over something that refuses files cancels too.
- The GNU build links and runs (41/41 unit tests, all live checks pass); the binary imports `msvcrt.dll`, no
  VCRUNTIME/MSVCP.

## Measured (release build, GNU toolchain, Windows 11, one 3440x1440 display, idle)

| | |
|---|---|
| Working set | ~59 MB (private ~55 MB) |
| Threads | 13 (12 without the mouse hook) |
| Binary | 32.4 MB (unstripped; the earlier MSVC build was 10.6 MB) |
| Idle CPU | ~0.5–0.7% of one core, with or without the hook |
| Island re-renders at idle | 0 over 10 s |

An earlier MSVC build measured ~1.5–2% idle CPU. The cause was never found (not our render loop, not the hook),
and the toolchain, the window design and machine load all differ now, so don't read 0.5–0.7% as a fix. It is
worth a profile before building on it, because an always-on app should be closer to 0%.

## Not done / known gaps

- Real jiggle thresholds are untuned (tested only with synthetic input). A text-selection drag also
  triggers the tray (the hook can't tell what is being dragged); the drop target refuses the payload
  and the tray folds itself away.
- Multi-monitor and non-100% DPI are handled in code (scale factor, the pill anchors to the monitor it opens on)
  but only tested on one 100% display.
- Glass: the tint and rim are untuned, and only tried over a stripe pattern and a dark desktop. Not verified:
  Windows 10 (no `DWMWCP_ROUND`, so square corners), behaviour with "Transparency effects" turned off, and
  light backdrops. The acrylic call is undocumented API and could change.
- Resizing a window every frame is not profiled, and the window trails the spring by a frame. No tearing showed
  in sampled frames, but it was not measured at speed.
- The shelf shows an extension badge, not the file's own icon or a thumbnail.
- The target decides copy, move or link (the default shell drop source offers all three), so a drag to a folder on
  the same drive moves the file, as in Explorer; the chip then leaves the shelf.
- No settings window or droplets yet. The tray icon is drawn in code and the exe itself has no icon resource.

## License

[MIT](LICENSE). The crates this builds on (GPUI and its dependency tree, 482 packages on Windows) are all
permissively licensed (MIT, Apache-2.0, BSD, Zlib, ISC and similar); a distributed binary should ship their notices.

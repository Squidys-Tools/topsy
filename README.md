# topsy

A Droppy-style notch/island for Windows, written in Rust on [GPUI](https://crates.io/crates/gpui).

This branch is a **feasibility spike** for the riskiest parts of the design:

1. A transparent, always-on-top, non-activating glass pill window with spring animation.
2. A global mouse hook that detects the "jiggle a drag" gesture.
3. Receiving a real file drag on that window.
4. A real blur behind the pill, in the pill's shape.

## What's here

| File | Purpose |
|---|---|
| `src/spring.rs` | Damped spring (Framer-Motion-style `stiffness/damping/mass`), fixed sub-step, retargetable mid-flight |
| `src/jiggle.rs` | Pure jiggle detector: direction reversals of a held-button drag inside a time window |
| `src/win32.rs` | Overlay window styles, acrylic glass, window geometry, `WH_MOUSE_LL` hook thread |
| `src/main.rs` | The island view: idle pill, expandable tray, file-drop target |
| `build.rs` | Copies one import library the GNU toolchain's bundled MinGW lacks (see below) |
| `rust-toolchain.toml` | Pins the GNU toolchain, `stable-x86_64-pc-windows-gnu` |

Right-click the pill to quit. Env vars for testing: `TOPSY_DEMO=1` (auto expand/collapse),
`TOPSY_LOG=1` (event trace on stderr), `TOPSY_EXIT_AFTER_MS=<n>`, `TOPSY_NO_HOOK=1` (don't install the
mouse hook), `TOPSY_GLASS=tint|solid|clear` (see [Glass](#glass); the default is the real blur).

## Running

```powershell
cargo run            # then hold the left mouse button on a file and jiggle it side to side
cargo test           # spring + jiggle unit tests
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
- `check-overlay.ps1` — only the pill is hit-testable, and launching doesn't change the foreground window.
  Run it on a release build (see above).
- `check-glass.ps1` — the blur proof described under [Glass](#glass). Shows a stripe window at the top of the
  screen for about 30 s.
- `trace-width.ps1` / `shot.ps1` — sample or screenshot the pill to look at the animation as composited
  (`trace-width.ps1` needs `TOPSY_GLASS=solid`).
- `measure-idle.ps1` / `count-renders.ps1` — idle footprint and re-render count.

## Findings

- **GPUI already registers an `IDropTarget` per window** and accepts only `CF_HDROP`, rejecting other drags.
  No custom OLE code is needed; use `on_drop::<ExternalPaths>` / `drag_over::<ExternalPaths>`.
- **`WindowOptions::focus` is ignored on Windows**; GPUI activates the window when it is created.
  `show: false` is *not* a workaround in gpui 0.2.2: it defers placement until `activate()` and the window is
  never painted. What works: create visible, apply `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` + topmost right away,
  and `SetForegroundWindow` back to whatever had focus. After that nothing activates the island, including
  expanding the tray mid-drag (checked in the e2e).
- **`SetCursorPos` does not reliably reach `WH_MOUSE_LL`**; test input must be `mouse_event`/`SendInput`.
- Low-level hooks do see button-up and moves during an OLE drag loop.
- The GNU build links and runs (11/11 unit tests, all live checks pass); the binary imports `msvcrt.dll`, no
  VCRUNTIME/MSVCP.

## Measured (release build, GNU toolchain, Windows 11, one 3440x1440 display, idle)

| | |
|---|---|
| Working set | ~59 MB (private ~55 MB) |
| Threads | 12 (11 without the mouse hook) |
| Binary | 32.4 MB (unstripped; the earlier MSVC build was 10.6 MB) |
| Idle CPU | ~0.5–0.7% of one core, with or without the hook |
| Island re-renders at idle | 0 over 10 s |

An earlier MSVC build measured ~1.5–2% idle CPU. The cause was never found (not our render loop, not the hook),
and the toolchain, the window design and machine load all differ now, so don't read 0.5–0.7% as a fix. It is
worth a profile before building on it, because an always-on app should be closer to 0%.

## Not done / known gaps

- Real jiggle thresholds are untuned (tested only with synthetic input).
- A text selection drag also triggers the tray (the hook can't tell what is being dragged); the drop target
  correctly refuses non-file payloads, so the tray should dismiss itself in that case. Not yet implemented.
- Multi-monitor and non-100% DPI are handled in code (scale factor, the pill anchors to the monitor it opens on)
  but only tested on one 100% display.
- Glass: the tint and rim are untuned, and only tried over a stripe pattern and a dark desktop. Not verified:
  Windows 10 (no `DWMWCP_ROUND`, so square corners), behaviour with "Transparency effects" turned off, and
  light backdrops. The acrylic call is undocumented API and could change.
- Resizing a window every frame is not profiled, and the window trails the spring by a frame. No tearing showed
  in sampled frames, but it was not measured at speed.
- No tray icon / settings / droplets yet.

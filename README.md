# topsy

A Droppy-style notch/island for Windows, written in Rust on [GPUI](https://crates.io/crates/gpui).

This branch is a **feasibility spike** for the three riskiest parts of the design:

1. A transparent, always-on-top, non-activating pill window with spring animation.
2. A global mouse hook that detects the "jiggle a drag" gesture.
3. Receiving a real file drag on that window.

## What's here

| File | Purpose |
|---|---|
| `src/spring.rs` | Damped spring (Framer-Motion-style `stiffness/damping/mass`), fixed sub-step, retargetable mid-flight |
| `src/jiggle.rs` | Pure jiggle detector: direction reversals of a held-button drag inside a time window |
| `src/win32.rs` | Overlay window styles, hit-test region, `WH_MOUSE_LL` hook thread |
| `src/main.rs` | The island view: idle pill, expandable tray, file-drop target |

Right-click the pill to quit. Env vars for testing: `TOPSY_DEMO=1` (auto expand/collapse),
`TOPSY_LOG=1` (event trace on stderr), `TOPSY_EXIT_AFTER_MS=<n>`, `TOPSY_NO_HOOK=1` (don't install the
mouse hook).

## Running

```powershell
cargo run            # then hold the left mouse button on a file and jiggle it side to side
cargo test           # spring + jiggle unit tests
```

Needs the MSVC build tools and a Windows SDK (GPUI compiles HLSL with `fxc`).

## Live-desktop checks (`scripts/`)

These drive the real desktop, so they move your cursor for a few seconds.

- `e2e-drag.ps1` — starts a genuine OLE file drag from a fixture window, jiggles it with injected input,
  verifies the tray expands, carries the drag onto the pill, and checks the drop arrives with the right file name.
  It probes that the pill is on screen first and aborts otherwise, so a drag is never carried to the top of the
  screen unless the pill is there to catch it.
- `check-overlay.ps1` — only the pill is hit-testable (the rest of the window is click-through) and the
  overlay doesn't take foreground focus.
- `trace-width.ps1` / `shot.ps1` — sample or screenshot the pill to look at the animation as composited.

## Findings

- **GPUI already registers an `IDropTarget` per window** and accepts only `CF_HDROP`, rejecting other drags.
  No custom OLE code is needed; use `on_drop::<ExternalPaths>` / `drag_over::<ExternalPaths>`.
- **`WindowOptions::focus` is ignored on Windows**; GPUI activates the window when it is created.
  `show: false` is *not* a workaround in gpui 0.2.2: it defers placement until `activate()` and the window is
  never painted. What works: create visible, apply `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` + topmost right away,
  and `SetForegroundWindow` back to whatever had focus. After that nothing activates the island, including
  expanding the tray mid-drag (checked in the e2e).
- **`SetWindowRgn` is enough for click-through** around the pill: no layered/transparent ex-styles. The
  region is widened while animating so spring overshoot isn't clipped, then tightened when settled.
- **`SetCursorPos` does not reliably reach `WH_MOUSE_LL`**; test input must be `mouse_event`/`SendInput`.
- Low-level hooks do see button-up and moves during an OLE drag loop.

## Measured (release build, Windows 11, one 3440x1440 display, idle)

| | |
|---|---|
| Working set | ~58 MB (private ~57 MB) |
| Threads | 12 (11 without the mouse hook) |
| Binary | 10.6 MB |
| Idle CPU | ~1.5–2% of one core, with *or* without the hook |
| Island re-renders at idle | 0 over 10 s |

The idle CPU is not our render loop (0 renders) and not the hook (same with `TOPSY_NO_HOOK=1`), so it is
somewhere in GPUI's Windows platform layer. **Not yet diagnosed** — worth a profile before building on it,
because an always-on app should be closer to 0%. Reproduce with `scripts/measure-idle.ps1` and
`scripts/count-renders.ps1`.

Spring animation as composited on screen (`scripts/trace-width.ps1`): the pill goes 168 → ~400 px in about
150 ms with a small overshoot (damping ratio ~0.68), then settles.

## Not done / known gaps

- Real jiggle thresholds are untuned (tested only with synthetic input).
- A text selection drag also triggers the tray (the hook can't tell what is being dragged); the drop target
  correctly refuses non-file payloads, so the tray should dismiss itself in that case. Not yet implemented.
- Multi-monitor and non-100% DPI are handled in code (scale factor) but only tested on one 100% display.
- Glass/blur: `WindowBackgroundAppearance::Blurred` blurs the whole window rect, not the pill shape.
  Not attempted here.
- No tray icon / settings / droplets yet.

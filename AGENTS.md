# topsy

topsy is a free, open-source take on [Droppy](https://getdroppy.app) for Windows: a glass island at the top of
the screen that opens into a file shelf when you jiggle a drag. Native Rust on [GPUI](https://crates.io/crates/gpui),
with raw Win32 through the `windows` crate. The README holds the design, findings and measurements; read the
sections that touch your change before you change it.

## What we never compromise on

1. **Light.** No webview, no JS runtime, no background polling. An always-on app should cost close to 0% CPU
   at idle and render nothing when nothing changes (`scripts/measure-idle.ps1`, `scripts/count-renders.ps1`).
   No animation that repaints continuously.
2. **Out of the way.** The island never takes focus, never activates, and is only hit-testable where the pill
   is. A change that steals the foreground window, even once at launch, is a bug.
3. **Native feel.** Real acrylic, spring motion, behaviour that matches Explorer (drag out, open, copy/move as
   the target decides). If it looks or acts like a web page in a window, it is wrong.
4. **Open.** MIT, permissive dependencies only. Check a new crate's license before adding it.

## Taste

Ambitious ideas, simple systems. Do not keep complexity because it exists, and do not add machinery because it
looks architectural. Find the real constraint, then the smallest model that makes the right behaviour
unsurprising. Measure twice, cut once, and YAGNI. Honor the developer's intent minimally and realistically.
These are good defaults, not hard rules; the developer's preferences override them. If one fights the task in
front of you, say so plainly and get sign-off before breaking it.

## Glossary

- **island**: the whole app window at the top of the screen. The OS window *is* the pill (see README, Glass).
- **pill**: the island collapsed. **tray**: the island expanded, showing the shelf.
- **shelf**: the files parked on the island (`src/shelf.rs`). **chip**: one shelf entry.
- **jiggle**: the gesture that opens the tray, side-to-side reversals of a held-button drag (`src/jiggle.rs`).
- **tray icon**: the notification-area icon and its menu (`src/tray.rs`). Not the same thing as the tray.
- **glass**: the acrylic blur behind the pill.

## Hard constraints

- **Rust only.** No C, C++ or other C-family code, and no C shims to get around a binding.
- **GNU toolchain** (`x86_64-pc-windows-gnu`, pinned in `rust-toolchain.toml`). Never switch to MSVC or add
  MSVC-only steps. `build.rs` exists because of this; read the README before touching it.
- The developer often tests from a machine without admin rights, using the CI artifact `topsy-windows-gnu`.
  CI is part of the product: keep it green and keep the artifact working.

## The ways to hurt yourself

1. **Killing by name.** Never `Stop-Process -Name topsy`, `taskkill /im topsy.exe`, or kill a PID found by
   matching a name or path. The developer's own topsy may be running (it can start with Windows), and other
   worktrees run theirs. Stop only a PID you captured when you started it, as the scripts do.
2. **Touching real user state.** `HKCU\...\CurrentVersion\Run\topsy` and the shell's `NotifyIconSettings` are
   the developer's live settings. Unit tests use a scratch key (`Software\topsy-test-<pid>-<tag>`, see
   `src/autostart.rs`); a live check that has to touch the real value must restore it in a `finally`.
3. **Pumping messages inside a GPUI handler.** Anything that runs a message loop (`DoDragDrop`, `ShellExecute`,
   modal dialogs, `TrackPopupMenu`) re-enters GPUI while the app is borrowed and aborts the process. Run it from
   a `cx.spawn` task or its own thread. Likewise, never resize or move the window inside `render`; do it from a
   foreground task (README, Glass).
4. **Fighting over the desktop.** The live checks in `scripts/` move the real cursor, inject input and put
   windows on screen. Never run two at once, and never run them from two worktrees at once.

## Hit every surface

The most common defect is a change that works on the path you tried and is missing everywhere else. Before
calling work done, walk this list and say which entries applied:

- **Entry points.** The pill (left click, right-click menu), the tray icon (left click, its menu) and the jiggle
  gesture often reach the same behaviour. Fixing one is not fixing the feature.
- **Reverse states.** A way in needs a way out and a way to see it. Add to the shelf needs remove and clear;
  turning startup on needs turning it off and a check mark.
- **Threads.** The UI thread, the mouse-hook thread, the tray thread and one-off worker threads each own their
  state. Events cross to the UI thread over `async_channel` and back to the tray thread as posted messages;
  shared statics are atomics, `OnceLock` or a `Mutex`, never `static mut`.
- **Displays.** Multi-monitor and non-100% DPI are real. Use the scale factor; never hard-code pixels as
  physical.
- **Test switches.** The `TOPSY_*` env vars (README, Running) must keep working; a new one goes in the README.
- **Docs.** Check whether the change makes the README wrong (see Documentation).

## Verifying

- Before you push, run `scripts/check.ps1`: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`
  and `cargo test`, the same gates as CI. While iterating, `cargo test <name>`.
- A cold build of GPUI takes about 10 minutes in a fresh worktree. Run long builds in the background.
- Keep logic in pure modules with no OS calls and no clock reads (`shelf`, `jiggle`, `spring`): take
  `now: Instant` or a step as an argument, so tests drive time instead of sleeping. Win32 and GPUI glue stays
  thin around them.
- Test behaviour that can break: gesture detection, spring physics, shelf rules, registry round trips, parsing.
  Leave out tests for what types already guarantee, trivial getters, and wiring. Extend an existing test before
  adding one, and test at the lowest layer that fails without the fix.
- A logic bug gets a failing test first, then the fix. A visual bug (spacing, colour, glass, motion) gets
  before/after captures (`scripts/shot.ps1`) instead of a test.
- No `sleep` in unit tests and no assertions on elapsed wall-clock time. A test that needs a timeout to pass
  is wrong. Flaky tests get fixed, not retried.
- Behaviour on the real desktop gets one pass with the live check for that area: `e2e-drag.ps1` (jiggle and
  drop), `e2e-shelf.ps1` (chips, drag out, open), `e2e-tray.ps1` (tray icon, menu, startup),
  `check-overlay.ps1` (focus and hit-testing), `check-glass.ps1` (blur). Judge focus and footprint on a release
  build only: a debug build is a console app and its console takes focus.

## Pull requests

- Never open a PR unless the developer asks, and never merge one unless they explicitly say to for that PR.
  Work goes on a branch off `main`.
- Conventional commit titles in plain language: `fix(shelf): chips survive a drag to the same drive`.
- Body: the problem in a sentence or two, how you fixed it, then how you verified it and what you could not
  check. End with the model and harness that did the work. Follow `.github/pull_request_template.md`.
- UI changes need before/after screenshots; motion or timing needs a short recording. Upload them to GitHub;
  never commit PR-only assets.
- One request is one PR.

## Documentation

- The README is the only doc. Its design sections (Glass, Findings, the drag-and-drop notes) are for decisions,
  constraints across components, and traps that are hard to find in the source. Before adding a paragraph, ask
  what a maintainer would get wrong without it; if reading the code answers it, leave it out.
- When a documented decision changes, rewrite or remove the text. Do not append a second account.
- Keep "Not done / known gaps" honest: remove a gap you closed, add one you found and left.
- Default to no code comment. Write one for a Win32 or GPUI quirk, a constraint or an invariant the code can't
  show, in a line or two. Don't restate names, narrate steps or explain the fix you just made; the PR does that.

## Plans and work artifacts

Do not commit plans, research notes, screenshots or agent scratch files. Keep them outside the worktree
(`/shots` is gitignored for captures). A merged PR is the record of the work.

# topsy

topsy is a free, open-source take on [Droppy](https://getdroppy.app) for Windows: a glass island at the top of
the screen that holds files, shows what is playing and keeps the clipboard. Native Rust on
[GPUI](https://crates.io/crates/gpui), with raw Win32 through the `windows` crate. The docs are indexed in
[docs/README.md](docs/README.md); read the internals pages that touch your change before you change it, and
[GPUI traps](docs/internals/gpui.md) before any GPUI code.

## What we never compromise on

1. **Light.** No webview, no JS runtime, no background polling. An always-on app should cost close to 0% CPU
   at idle and render nothing when nothing changes (`scripts/measure-idle.ps1`, `scripts/count-renders.ps1`).
   No animation that repaints continuously.
2. **Out of the way.** The island never takes focus, never activates, and is only hit-testable where the notch
   is. A change that steals the foreground window, even once at launch, is a bug.
3. **Native feel.** Real acrylic, spring motion, behaviour that matches Explorer (drag out, open, copy/move as
   the target decides). If it looks or acts like a web page in a window, it is wrong.
4. **Open.** MIT, permissive dependencies only. Check a new crate's license before adding it. The exception is
   a mature tool topsy downloads on first use (FFmpeg, ImageMagick, pdfium): it may carry another licence, since
   it is fetched from its own release, never bundled, and nothing of it is compiled into topsy.

## Taste

Ambitious ideas, simple systems. Do not keep complexity because it exists, and do not add machinery because it
looks architectural. Find the real constraint, then the smallest model that makes the right behaviour
unsurprising. Measure twice, cut once, and YAGNI. Honor the developer's intent minimally and realistically.

- Complexity belongs at the boundary. Win32 glue absorbs the mess, `crates/model` stays pure, views stay dumb.
- These are good defaults, not hard rules; the developer's preferences override them. If one fights the task in
  front of you, say so plainly and get sign-off before breaking it.

## Glossary

- **you**: the agent reading this file. **we**: the developer and the agents working on topsy. **user**: someone
  running topsy.
- **island**: the whole app window at the top of the screen. The OS window *is* the notch
  ([Notch](docs/internals/notch.md)).
- **notch**: the island's shape, hanging from the top edge. **hidden**: folded away, the resting state. **peek**: the
  small notch revealed by resting the cursor on the top edge. **tray**: the island expanded, showing the shelf.
- **shelf**: the files parked on the island (`crates/model/src/shelf.rs`). **chip**: one shelf entry.
- **jiggle**: side-to-side reversals of a held-button drag (`crates/model/src/jiggle.rs`); it brings up the basket.
- **basket**: the floating file tray a jiggle brings up, in a window of its own (`src/basket/`).
- **tray icon**: the notification-area icon and its menu (`src/tray.rs`). Not the same thing as the tray.
- **glass**: the acrylic blur behind the notch.

## Hard constraints

- **Rust only.** We write no C, C++ or other C-family code, and no C shims to get around a binding. Running
  mature C code is fine: a downloaded tool run as a process, or a DLL such as pdfium loaded through a Rust crate.
  Prefer that to rewriting a codec or format from scratch.
- **GNU toolchain** (`x86_64-pc-windows-gnu`, pinned in `rust-toolchain.toml`). Never switch to MSVC or add
  MSVC-only steps. `build.rs` exists because of this; read [GNU toolchain](docs/internals/toolchain.md) before
  touching it.
- The developer often tests from a machine without admin rights, using the CI artifact `topsy-windows-gnu`
  (uploaded for `main` and for runs started by hand, not for PRs). CI is part of the product: keep it green and
  keep the artifact working.

## The ways to hurt yourself

1. **Killing by name.** Never `Stop-Process -Name topsy`, `taskkill /im topsy.exe`, or kill a PID found by
   matching a name or path. The developer's own topsy may be running (it can start with Windows), and other
   worktrees run theirs. Stop only a PID you captured when you started it, as the scripts do.
2. **Touching real user state.** `HKCU\...\CurrentVersion\Run\topsy`, `HKCU\Software\topsy` and the shell's
   `NotifyIconSettings` are the developer's live settings. Unit tests use a scratch key
   (`Software\topsy-test-<pid>-<tag>`, see `src/autostart.rs`); a live check that has to touch the real value
   must restore it in a `finally`.
3. **Pumping messages inside a GPUI handler.** Anything that runs a message loop (`DoDragDrop`, `ShellExecute`,
   modal dialogs, `TrackPopupMenu`) re-enters GPUI while the app is borrowed and aborts the process. Run it from
   a `cx.spawn` task or its own thread. Likewise, never resize or move the window inside `render`; do it from a
   foreground task ([GPUI traps](docs/internals/gpui.md)).
4. **Fighting over the desktop.** The live checks in `scripts/` move the real cursor, inject input and put
   windows on screen. Never run two at once, and never run them from two worktrees at once.

## Hit every surface

The most common defect is a change that works on the path you tried and is missing everywhere else. Before
calling work done, walk this list and say which entries applied:

- **Entry points.** The notch (the top-edge reveal, left click, right-click menu), the tray icon (left click,
  its menu), the basket and the jiggle gesture often reach the same behaviour. Fixing one is not fixing the
  feature.
- **Reverse states.** A way in needs a way out and a way to see it. Add to the shelf needs remove and clear;
  turning startup on needs turning it off and a check mark.
- **Threads.** The UI thread, the mouse-hook thread, the tray thread and one-off worker threads each own their
  state. Events cross to the UI thread over `async_channel` and back to the tray thread as posted messages;
  shared statics are atomics, `OnceLock` or a `Mutex`, never `static mut`.
- **Displays.** Multi-monitor and non-100% DPI are real. Use the scale factor; never hard-code pixels as
  physical.
- **Test switches.** The `TOPSY_*` env vars must keep working; a new one is a field in `src/config.rs` and a row
  in [Running](docs/operations/running.md).
- **Docs.** Check whether the change makes a doc wrong (see Documentation).

## How it works

Win32 delivers the outside world on threads of topsy's own: the low-level mouse and keyboard hooks, the tray
icon's hidden window, the media session, the clipboard listener, power and WMI events. Each thread does only its
Win32 part and sends a plain event to the UI thread over an `async_channel`. There a foreground task hands it to
the pure rules in `crates/model` (the jiggle detector, the edge, the shelf, the spring), and the view renders what
they decide. Work that blocks or pumps messages goes back out to a `cx.spawn` task or a thread.

## Where code lives

- `crates/model`: the rules, a crate of their own (`model::` in the app). Pure and unit tested: no `windows`,
  `gpui`, clock or environment. A feature's logic starts here with its tests; the glue that feeds it events and
  draws its result stays thin. `cargo test -p topsy-model` runs them in seconds, without building GPUI. A helper
  only tests use is `#[cfg(any(test, feature = "test-support"))]`, so the app's tests can reach it too.
- `src/win32/`: Win32 glue GPUI doesn't expose, a file per job (`window`, `dragdrop`, `hook`, `shell`, `menu`,
  `clipboard`, …).
- `src/island/`: the island view, its modes and springs, the shelf's chips, the tray's layout and pages, and the
  entry points features reach it through. `src/app.rs`: the app's wiring. `src/config.rs`: every `TOPSY_*` switch.
- One file or folder per feature beside them: `activity/`, `basket/`, `clipboard/`, `now_playing/`, `settings/`,
  `transform/`, `widgets/`, `hud.rs`, `tray.rs` (the tray icon). Each module's doc comment says what it owns. A
  feature keeps its logic on its own types and never adds methods to `Island`; how a feature is wired in, and every
  place a new one touches, is in [Features](docs/internals/features.md).

### Structure

- No file over 500 lines. When a file nears the limit, split it by job, not by size.
- `scripts/check-structure.ps1` (in `check.ps1` and CI) enforces that, that only `src/island/` and `src/app.rs`
  add methods to `Island`, and the purity of `crates/model`. `src/island/mod.rs` has a ceiling of its own there
  that only comes down: lower it when the file shrinks, never raise it.

## Rust

Rules for new and touched code. Don't sweep old code to match.

- Unused code is deleted, not silenced. If a lint must be silenced, use `#[expect(lint, reason = "...")]`, not
  `#[allow]`.
- No `unwrap()` or `expect()` outside tests: propagate with `?`, log, or encode the invariant in types.
- Never discard an error silently with `let _ =`: propagate it, or `.log_err()` it (`crate::log`) when failing
  changes nothing for the caller. A Win32 call whose normal success reads as failure (`GlobalUnlock`,
  `GlobalFree`) or returns no error at all gets `#[expect(clippy::let_underscore_must_use, reason = "...")]`.
  Removing a file that may not be there is `.missing_ok().log_err()`. Clippy enforces all of this.
- Spell out every enum variant in a `match`; avoid `_` arms and `matches!` on our own enums, so a new variant is a
  compile error everywhere it matters.
- Prefer let chains (`if let Some(x) = a && cond`) to nested `if let`.
- Full-word names (`queue`, not `q`); GPUI's `cx` and `window` are the exceptions.
- Imports at the top of the file, never inside functions.
- Narrowest visibility: private, then `pub(crate)`, rarely `pub`.
- A struct and its `impl` stay together unless deliberately split.
- Default to no comment. Write one only for a constraint, a Win32 or GPUI quirk, or an invariant the code can't
  show: one line where you can, four at most. Don't restate names, narrate steps or explain the fix you just
  made; the PR does that.
- Write for the reader: comments, docs and PR text never mention the conversation, earlier attempts or session
  history.
- Dependency changes one at a time with `cargo update --precise`; never update everything (GPUI breaks easily).
- Committed scripts live in `scripts/` and each states its usage in its header; one-off scratch scripts stay
  outside the worktree.

## Verifying

- Locally, run `cargo fmt` and the tests for what you changed (`cargo test <name>`). CI runs full clippy and the
  full test suite (`scripts/check.ps1` holds the same gates if you want them locally). When CI fails, read its
  log and fix the cause. Never assume a clippy warning or failing test is pre-existing; `main` is green.
- A cold build of GPUI takes about 10 minutes in a fresh worktree. Run long builds in the background.
- Keep logic in `crates/model`, with no OS calls and no clock reads: take `now: Instant` or a step as an argument,
  so tests drive time instead of sleeping.
- Test behaviour that can break: gesture detection, spring physics, shelf rules, registry round trips, parsing.
  Leave out tests for what types already guarantee, trivial getters, and wiring. Extend an existing test before
  adding one, and test at the lowest layer that fails without the fix.
- A logic bug gets a failing test first, then the fix. A visual bug (spacing, colour, glass, motion) gets
  before/after captures (`scripts/shot.ps1`) instead of a test.
- No `sleep` in unit tests and no assertions on elapsed wall-clock time. A test that needs a timeout to pass
  is wrong. Flaky tests get fixed, not retried.
- Behaviour on the real desktop gets one local pass, one check at a time, with the live check for that area
  ([Live checks](docs/operations/live-checks.md)). Judge focus and footprint on a release build only: a debug
  build is a console app and its console takes focus.

## Pull requests

- Never open a PR unless the developer asks, and never merge one unless they explicitly say to for that PR.
  Work goes on a branch off `main`.
- Conventional commit titles in plain language: `fix(shelf): chips survive a drag to the same drive`.
- Body: the problem in a sentence or two, how you fixed it, then how you verified it and what you could not
  check. End with the model and harness that did the work. Follow `.github/pull_request_template.md`.
- UI changes need before/after screenshots; motion or timing needs a short recording. Upload them to GitHub;
  never commit PR-only assets.
- One request is one PR.
- When babysitting a PR, use the harness's PR watch tool and end your turn; it wakes you on checks, comments or
  conflicts. Never poll or sleep.

## Documentation

Most code changes need no doc change. Agents can read the code.

- `docs/user/` helps users get things done, in the product's voice. A concise section per major feature: what it
  does, how to start, anything unintuitive, settings paths. No implementation details, no contributor tooling,
  no catalogue of buttons, layouts or UI states. A UI tweak needs no entry; a new control needs no page.
- `docs/internals/` holds architectural decisions and their reasons, constraints across components, and traps
  hard to find in the source. Before adding a paragraph, ask what a maintainer would get wrong without it; if
  reading the code answers it, leave it out. No file catalogues, no field lists, no PR summaries. A local reason
  goes in a code comment instead.
- `docs/operations/` holds building, CI, the live checks and the measurements.
- When a documented decision changes, rewrite or remove the text. Never append a second account.
- Keep [known gaps](docs/internals/known-gaps.md) honest: remove a gap you closed, add one you found and left.

## Rules hygiene

Every session reads this file, so keep it high-signal. A new rule must be non-obvious (someone who knows the code
would still get it wrong), have come up more than once, and be concrete enough to act on. During normal work,
don't edit AGENTS.md: propose additions under a **Suggested AGENTS.md additions** heading in the PR description.
Fixing or clarifying an existing rule is always welcome.

## Plans and work artifacts

Do not commit plans, research notes, screenshots or agent scratch files. Keep them outside the worktree
(`/shots` is gitignored for captures). A merged PR is the record of the work.

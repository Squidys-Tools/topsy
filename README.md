# topsy

A Droppy-style notch/island for Windows, written in Rust on [GPUI](https://crates.io/crates/gpui).

It started as a **feasibility spike** for the riskiest parts of the design, and all four held up:

1. A transparent, always-on-top, non-activating glass pill window with spring animation.
2. A global mouse hook that detects the "jiggle a drag" gesture.
3. Receiving a real file drag on that window.
4. A real blur behind the pill, in the pill's shape.

On top of that it is now a usable v1: the island is a **notch** that stays out of sight until you reach for
it (see [Notch](#notch)), the tray holds a **shelf** of files you can drag back out, there is a
**tray icon** with a menu, **start with Windows**, a **Send to** entry in Explorer, and a **Quit** item. See
[Shelf, tray icon and startup](#shelf-tray-icon-and-startup). It also shows what is playing, with its controls
(see [Now Playing](#now-playing)), and keeps a **clipboard history** you open with Ctrl+Alt+V (see
[Clipboard history](#clipboard-history)). Volume, brightness and battery changes drop the notch down as a
**HUD** (see [HUDs](#huds)). Files on the shelf can be **transformed**: compressed, converted, resized,
trimmed, turned into a GIF or a PDF, stripped of metadata or cut out of their background (see
[Transform](#transform)).

## What's here

`src/model/` holds the rules, pure and unit tested: no Win32, no GPUI, no clock, no environment
(`scripts/check-structure.ps1` checks).

| File | Purpose |
|---|---|
| `src/model/spring.rs` | Damped spring (Framer-Motion-style `stiffness/damping/mass`), fixed sub-step, retargetable mid-flight |
| `src/model/jiggle.rs` | Jiggle detector: direction reversals of a held-button drag inside a time window |
| `src/model/notch.rs` | The notch's outline: body, ears and corner radius for a given size |
| `src/model/float.rs` | Where the floating tray opens: centred under the pointer, kept whole on its monitor's work area |
| `src/model/edge.rs` | Reaching for the hidden notch: the cursor arriving on the top row above it; what counts as fullscreen |
| `src/model/shelf.rs` | The shelf: files parked on the island, and its save format |
| `src/model/archive.rs` | Naming a chip's zip (`report.zip`, `report (2).zip`) and the entries inside it |
| `src/model/media.rs` | Now Playing: the media session's events reduced to what the tray and the peek show |
| `src/model/levels.rs` | Now Playing's level bars: peak readings to bar heights, and the colour they take from the art |
| `src/model/clipboard.rs` | Clipboard history: adding a copy (a repeat moves up), pins, expiry, the cap, and its save format |
| `src/model/clip_kind.rs` | What a copy is (text, link, colour, code, image, files) and how its card describes it; search |
| `src/model/highlight.rs` | A code card's colours: keywords, strings, comments and numbers, in one pass that knows no language |
| `src/model/favicon.rs` | Which sites a link card may ask for an icon, where a redirect or a page's icon link may lead, and which icon a page names |
| `src/model/hud.rs` | The HUDs: how much of the bar a level lights, and which battery changes are worth showing |
| `src/model/dib.rs` | Reading a copied bitmap (`CF_DIB`/`CF_DIBV5`) to RGBA, and writing one back to paste |
| `src/model/transform.rs` | Transform: which actions suit which files, the panel's summary line, and naming results and renames |
| `src/model/transform_card.rs` | What a Transform result card says: size before and after, then the sides or the name |
| `src/model/image_ops.rs`, `image_geometry.rs`, `magick.rs`, `size_search.rs` | Image Transforms: formats and picking one, sizes, crops and turns, ImageMagick's and oxipng's command lines, landing under a size |
| `src/model/heif.rs` | Writing HEIC: x265's command line, lifting the coded frame out of FFmpeg's MP4, and the HEIF boxes around it |
| `src/model/orientation.rs`, `exif.rs`, `jpeg/` | Turning a photo once (its EXIF orientation and the user's turn composed), and JPEGs turned and stripped losslessly without a decode |
| `src/model/pdf.rs` | PDF Transforms: Combine's page sizes, Compress's presets, how far a picture inside may shrink |
| `src/model/video/` | Video and audio Transforms: options, size budgets, trim ranges, FFmpeg's command lines, reading ffprobe and progress |
| `src/model/matte.rs`, `download.rs` | Remove background's arithmetic (the model's input, its mask to a clean matte), and fetching a big file once, resumably |
| `src/model/tray_cause.rs` | Why the tray is open, and which events fold it early |
| `src/config.rs` | Every `TOPSY_*` switch, read once at startup |
| `src/backdrop.rs` | The glass: the compositor's blurred backdrop behind GPUI's content, clipped to the notch's shape |
| `src/win32/` | Win32 glue GPUI doesn't expose: `window` (overlay styles, acrylic fallback, geometry, fullscreen), `dialog` (the settings window's dark caption and raising it), `dragdrop` (the drop-target wrapper that reports refused drags, dragging files out), `hook` (`WH_MOUSE_LL` thread), `shell` (opening files, showing them in Explorer), `menu` (a popup menu on a thread of its own), `meter` (the output device's peak meter), `sendto` (the Send to shortcut), `inbox` (paths handed over by a second topsy), `clipboard` (the clipboard listener and the Ctrl+Alt+V hotkey), `paste` (putting an entry back on the clipboard and pasting it), `http` (one https GET through WinHTTP), `volume` (taking the volume keys and stepping the volume), `brightness` (WMI's brightness events), `battery` (power notifications) |
| `src/chip.rs` | A chip: how it is drawn, opening it, dragging it out, and its right-click actions |
| `src/archive.rs` | *Compress to ZIP file*: a chip's file or folder zipped beside it ([`zip`](https://crates.io/crates/zip), MIT) |
| `src/icon.rs` | A chip's picture: the shell's thumbnail or icon for a file, as Explorer shows it |
| `src/state.rs` | The shelf's save file and the thread that writes it |
| `src/tray.rs` | The notification-area icon and its menu, on their own thread |
| `src/clipboard.rs` | Clipboard history on disk, owned by one thread; the history setting and how long it is kept |
| `src/clipboard/strip.rs` | The clipboard strip: the window Ctrl+Alt+V opens, its search, tabs, keys and card menu |
| `src/clipboard/card.rs` | One card in the strip, drawn after Droppy's |
| `src/clipboard/pictures.rs` | The pictures the cards show (app icons, images, file thumbnails, site icons), loaded off the UI thread |
| `src/clipboard/favicon.rs` | A link card's site icon: fetched once per host and kept beside the history |
| `src/hud.rs` | The HUDs: the notch dropping down to show a new volume, brightness or battery state; the *Volume HUD* setting |
| `src/settings.rs` | The on/off settings, one list shared by the menu and the settings window |
| `src/settings_window.rs` | The settings window: a switch per setting, how long clipboard history is kept, the version, Quit |
| `src/transform/` | The Transform panel (`mod`, `view`, `options`, `widgets`, `clip` for the trim range), its result cards (`cards`), its worker (`job`), the bridge to the engines (`engines`, `pictures`), the engines (`image`, `pdf`, `video`, `background`), and the downloaded tools (`fetch`, `tools`) |
| `src/app.rs` | The app's wiring: opens the island and connects the threads, tray icon and settings window to it |
| `src/autostart.rs` | Start with Windows: one `HKCU\...\Run` value |
| `src/media.rs` | The system's media session (WinRT `GlobalSystemMediaTransportControlsSessionManager`) on its own thread |
| `src/now_playing.rs` | Now Playing's player (art, level bars, progress bar, buttons) and the peek's hint |
| `src/tray_view.rs` | The tray's layout: the player above the shelf's header and chips |
| `src/look.rs` | The *Black notch* setting: one `HKCU\Software\topsy` value |
| `src/frame.rs` | The island's window and shape: where it sits (the notch, or the floating tray), its size, fill, ears and rim, kept in step with the spring |
| `src/main.rs` | The island view: hidden, peeking or open as the tray with the shelf; file-drop target |
| `build.rs` | Copies one import library the GNU toolchain's bundled MinGW lacks, and builds the exe's icon (see below) |
| `rust-toolchain.toml` | Pins the GNU toolchain, `stable-x86_64-pc-windows-gnu` |

Right-click the notch or the tray icon for the menu (Quit is in it). Env vars for testing: `TOPSY_DEMO=1`
(auto expand/collapse), `TOPSY_LOG=1` (event trace on stderr), `TOPSY_EXIT_AFTER_MS=<n>`, `TOPSY_NO_HOOK=1` (don't
install the mouse hook; the notch then rests peeking, as nothing could reveal it), `TOPSY_NO_TRAY=1` (no tray icon
and no menu, so right-clicking the notch quits; start-with-Windows is left alone), `TOPSY_NO_MEDIA=1` (don't watch
the system's media session; no Now Playing), `TOPSY_NO_KEYS=1` (leave the volume keys to Windows, whatever the
*Volume HUD* setting says), `TOPSY_IDLE=peek|tray` (what the
notch rests as, instead of hidden; the live checks use it),
`TOPSY_SEED=<path;path>` (start with these files on the shelf), `TOPSY_TRANSFORM=<path;path>` (open the
Transform panel on these files at launch), `TOPSY_STATE=<path>` (the shelf's save file,
`%LOCALAPPDATA%\topsy\shelf.txt` by default; set empty to not save), `TOPSY_CLIPBOARD=<dir>` (where clipboard
history is kept, `%LOCALAPPDATA%\topsy\clipboard` by default; set empty for none. A run with `TOPSY_STATE` set keeps
none unless this says where, so a live check never touches the real history), `TOPSY_GLASS=acrylic|tint|solid|clear` (forces
a fill and ignores the *Black notch* setting; see
[Glass](#glass); the default is the notch-shaped blur).

## Running

```powershell
cargo run            # then hold the left mouse button on a file and jiggle it side to side
cargo test           # spring, jiggle, shelf and its save file, tray icon and start-with-Windows unit tests
scripts/check.ps1    # fmt, clippy, tests and structure, the same gates as CI; run it before pushing
```

This builds with the **GNU toolchain** (`x86_64-pc-windows-gnu`), pinned in `rust-toolchain.toml`. No MSVC.
You need:

- A MinGW-w64 `gcc` on `PATH` (e.g. `scoop install gcc`, or MSYS2). `build.rs` asks it where `libktmw32.a`
  lives: rustc's bundled MinGW ships only some of the Windows import libraries and a GPUI dependency links
  `ktmw32`. Only that one file is copied, so the build doesn't mix two MinGW runtimes. Pointing rustc's
  linker at the system gcc instead does not work: scoop's gcc has no `libgcc_eh.a`. The same MinGW's `windres`
  turns the icon `build.rs` draws (`src/model/pill.rs`, written as a binary `.res`) into an object linked into the
  exe, so no `.rc` script and no preprocessor are involved. GPUI's windows and the tray icon load it as resource 1.
- rustup's `cargo` first on `PATH`. A standalone Rust install (such as an MSVC one) ignores `rust-toolchain.toml`,
  so `build.rs` stops an MSVC build rather than let it through without the icon.
- The Windows SDK, for `fxc.exe` (GPUI compiles its HLSL shaders with it; set `GPUI_FXC_PATH` if it isn't found).

Use a release build to judge anything about focus or footprint. A debug build is a console app, and the
console window it opens takes foreground focus at launch. Release builds use the GUI subsystem.

### No toolchain, or no admin rights?

CI (`.github/workflows/build.yml`) builds the same release exe on a GitHub Windows runner, which has the Windows
SDK and a MinGW gcc. Pull requests only run the gates; the exe is uploaded for every push to `main` and for a run
you start by hand: `gh workflow run build.yml --ref <branch>`, or **Run workflow** on the repository's **Actions**
tab. Download the `topsy-windows-gnu` artifact from that run: no Rust, MinGW or SDK needed to run it. A cold build
takes about 20 minutes.

To build locally without installing the SDK, GPUI only needs one file from it: copy `fxc.exe` (from
`Windows Kits\10\bin\<version>\x64`) anywhere and set `GPUI_FXC_PATH` to it.

## Shelf, tray icon and startup

- **Shelf.** Drop files on the notch and they are parked in the tray as chips (the file's thumbnail or icon as
  Explorer shows it, the name, and a cross to remove it from the shelf; the file itself is never touched). The chips are references, not copies.
  Drag a chip out and drop it on any window, as you would from Explorer; click a chip to open it; **Clear**
  empties the shelf. Right-click a chip for *Show in folder* (Explorer opens with it selected), *Copy as path*
  (quoted, as Explorer copies it), *Compress to ZIP file* and *Remove from shelf*. The zip is written beside the
  original, named as Explorer would (`report.zip` for `report.pdf`, a folder keeps its name and is zipped whole,
  `report (2).zip` if the name is taken, never overwriting), on a worker thread, and is parked on the shelf when
  it is done; if it fails (a locked file, no room), the half-written zip is deleted and no chip appears. Links and
  junctions inside a folder are left out of its zip. There is no rename: the island never takes focus, so it can't
  hold a text field. A chip leaves the shelf once a drop is accepted. Files that were moved or deleted since they
  were parked are dropped from the shelf when the tray opens (checked off the UI thread). The shelf is saved to
  `%LOCALAPPDATA%\topsy\shelf.txt` on every change and comes back on the next start. Startup never looks at the saved files (a dead share would
  stall launch): chips return as `…` and are settled off the UI thread, which drops entries whose files are gone.
- **Tray icon.** A small glass-blue pill in the notification area, the exe's own icon (drawn in code, no asset). Left click opens
  or closes the shelf. Right click, or right-click the notch itself, opens the menu: *Show shelf*, *Clear shelf (N)*,
  *Black notch*, *Start with Windows* and *Show in Send to* (each checked when on), *Settings…* and *Quit topsy*. The icon lives on its own thread with a hidden window,
  re-adds itself when Explorer restarts, and is removed on Quit. On Windows 11 a new icon starts in the overflow
  flyout; drag it out to keep it visible.
- **Settings window.** *Settings…* opens a small dark window, *topsy settings*, with a switch for each setting,
  the version and *Quit topsy*. There is one at a time: asking again brings it to the front. It is an ordinary
  window and takes focus when it opens, which the island never does; closing it leaves the app running. The
  menu and the window are both built from the list in `src/settings.rs` and read each value from where it is
  saved, so a switch in one shows in the other; a new toggle is a variant there and an arm in each of its
  matches, plus what the island does about it (`Island::on_setting`).
- **Black notch** draws the notch plain black instead of glass, at once, and is kept in
  `HKCU\Software\topsy\Look` (`black`; absent means glass).
- **Start with Windows** writes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\topsy` (the quoted path of
  the running exe): per-user, no admin, nothing to uninstall. It is off until you turn it on from the menu. If
  the exe has moved since, the stored path is repaired at launch; an entry is never created by itself.
- **Show in Send to** puts `topsy.lnk` in the user's SendTo folder (`%APPDATA%\Microsoft\Windows\SendTo`), so
  Explorer's *Send to* menu lists topsy. Off until you turn it on; like startup, the shortcut is repaired at launch
  only if the exe it points at is gone. Explorer starts `topsy.exe` with the chosen paths. If topsy is already
  running, the new process hands them over (`WM_COPYDATA` to a message-only window, `topsy.inbox`) and exits, and
  the running one parks them and opens the tray; otherwise the new process starts with them on its shelf.

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
  handler (a UAC prompt, a cold network path) from freezing the island. A chip's menu (`TrackPopupMenu`) and
  *Show in folder* do the same: the menu is owned by a hidden window on its own thread, which is made
  foreground so the menu closes on a click elsewhere, and hands foreground back to the window that had it.

## Now Playing

A tray opened to look (a click on the notch, the tray icon) leads with a player for the system's current media
session (the one the volume flyout shows: Spotify, a browser tab, the Media Player), laid out after Droppy's: the
art, title and artist with level bars beside them; elapsed time, a progress bar and the time left; then previous, play/pause and next, greyed
where the app doesn't take them. The shelf follows under it once it holds something. A tray opened for a drag
(a jiggle, a drag up to the edge) or by a drop is about the shelf and leaves the player out (`TrayCause::sections`).
While something is playing (not paused) the peek shows its art, title and level bars instead of the shelf count,
with the count beside it. Stopped or no session shows nothing.

- **Event-driven only.** `src/media.rs` subscribes to `CurrentSessionChanged`, `MediaPropertiesChanged`,
  `PlaybackInfoChanged` and `TimelinePropertiesChanged`. WinRT raises them on thread-pool threads, whose handlers only wake one `topsy-media`
  thread; it reads the session (and decodes the art, scaled down by WIC) and sends the result to the UI thread
  over a channel. Between events nothing runs (but for the progress bar and the level bars while they are on screen, below),
  and a change while the notch is hidden doesn't render.
- **The progress bar** is carried forward from the app's last report of its position, which apps send rarely
  and not always on pause, so `model::media` holds the position where it got to when playback stops. While the
  player is open (a tray opened to look) and the track playing, one redraw is booked for each new second;
  anything else books none.
- **The level bars** move with the default output device's peak meter (`IAudioMeterInformation`, what the
  volume flyout draws), so they follow everything playing, not only the session shown; capturing the app's own
  audio would need a loopback stream, far heavier. While the peek or the player shows them and the track plays,
  the meter is read about 12 times a second (each read is a whole redraw of the island, a few ms of CPU; measured
  at about 2.5–3% of a core on top of idle), and reopened each second to follow a change of output device.
  Otherwise it is let go and nothing is booked. `model::levels` gives the bars an automatic gain (a quiet track fills them as a loud one does) and ripples each beat out from the middle.
  They take a colour averaged from the art's vivid pixels, lifted to read on black, or white when the art has none.
- **Buttons** go to the same thread, which calls the session's `Try*Async` without waiting; the glyph changes
  when the app reports its new state.

## Clipboard history

Copies are kept: text, links, colours, code, images and files, each with the app it came from (not blank
copies, text over 1 MB, or ones marked private). Ctrl+Alt+V opens the **strip**, a black band along the bottom
of the monitor under the cursor, laid out after Droppy's: a card per copy, newest first, with a pill naming the app, the kind and how long ago, and a line of detail at its
foot. Type to search; Left/Right to pick; Enter pastes into the window that was in front; Ctrl+C only copies;
Ctrl+P pins; Delete removes; Tab shows the pinned ones; Esc clears the search, then closes. Right-click a card,
or its ⋯ button, for the same actions. Code cards are coloured (keywords, strings, comments, numbers); link cards
show the site's icon. History is kept 7 days by default (1, 7, 30 days or forever in the settings window, whose
*Clear unpinned* empties it but for the pins); pinned entries never expire. It can be switched off from the menu
or the settings window.

- **The strip takes focus**, unlike the island: it is typed into. It remembers the window that was in front when
  the hotkey fired, hands focus back to it on Esc and after a paste, and closes on a click elsewhere. Paste puts
  the entry on the clipboard, waits up to 500 ms for that window to be in front again, and only then sends
  Ctrl+V: keys sent sooner land wherever the focus is. If it never comes back no keys are sent, and the entry
  stays on the clipboard. Being a copy, a paste moves the entry to the front.
- **A card's menu opens on the button's release.** Opened on the press, the release still goes to the strip and
  the menu never gets the keyboard: Esc and its access keys do nothing.
- **Nothing polls.** The listener (`AddClipboardFormatListener`) wakes on each copy; the store thread wakes on a
  command or when the oldest entry is due to expire. Copies marked private
  (`ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` = 0, as password managers set
  them) are never read. Many apps announce one copy twice, so the same thing copied again within 2 s is one copy.
- **On disk** each entry is `<id>.json` in the history folder, an image's pixels `<id>.png` beside it, each
  written through a temp file. A kill between the two can leave a `.tmp` or an image with no entry; the store
  removes those (and a half-written site icon) when it starts. At most 2000 unpinned entries are kept, and text over 1 MB is not. One topsy keeps
  a folder; a second one started on it keeps no history, since both would number and expire the same files.
- **Site icons are the only network traffic.** A link card that is shown asks its site for `/favicon.ico`, else
  the icon its home page names, once per host, through WinHTTP (the system's TLS, no crate of our own). Only
  https and public names: never a bare name, an address, a local-network suffix, or a name that resolves to a
  private address, and a redirect or icon link is vetted the same way. The icon is kept as a small PNG in
  `icons\<host>.png` beside the history; an empty file there means the site has none, so it isn't asked again.

## HUDs

When the volume, the brightness or the battery changes, the notch drops down a little wider and shows it, laid
out after Droppy's: an icon on the left and a bar of 16 dashes lit up to the level, or for the battery what
happened, the charge and a battery cell. It folds away 1.5 s after the last change (later if the cursor is on
it). Over an open tray it shows as a pill across the top instead. The battery shows when it is plugged in, when
it falls below 20% and 10% (or is unplugged below 20%), and when it is full on the charger.

- **topsy takes the volume keys** (the *Volume HUD* setting, on by default): a low-level keyboard hook on a
  thread of its own swallows VolumeUp, VolumeDown and Mute, so Windows never shows its own volume flyout, and
  hands them to a thread that steps the default output device by its own step (`IAudioEndpointVolume`), as
  Windows does; up and down unmute. Every other key passes straight through. Switched off, the hook is removed
  and the keys are Windows' again. Windows' flyout can't be hidden any other way: it is the shell's, not an app's.
- **Brightness** comes from WMI's `WmiMonitorBrightnessEvent`, which the display driver raises on each change;
  a thread blocks on it. Windows' own brightness flyout still shows: the keys are the laptop's firmware's.
- **Battery** changes arrive as power-setting callbacks (`PowerSettingRegisterNotification`). Nothing polls; a
  machine without a battery registers nothing.

## Transform

*Transform* on a chip's menu, or in the shelf's header, opens a panel laid out after Droppy's: the files at the
top, a tile for each action that suits them, the chosen action's options, and one button. Results are saved
beside the originals (`photo (compressed).jpg`, `report (page 2).png`, `Combined.pdf`), numbered rather than
overwriting, and parked on the shelf. While it works, the panel steps aside for a column of result cards where
it stood, as Droppy's does: one per file (one for a zip, a PDF or a rename), blurred with a progress bar while
waiting or working, then sharp with the size before and after and the picture's sides. A card opens its result;
the panel closes once the run is done, and the cards go by themselves six seconds later, unless the cursor is
on them. Only Rename touches the originals; a card's stop button, or a file that fails, stops the work,
deletes what it had made and brings the panel back.

- **Images:** Compress (by quality or to a size, as JPEG, PNG, HEIC, WebP or AVIF, or *Most compatible*: JPEG, or
  PNG when there is transparency; a largest side), Convert (JPEG, PNG, HEIC, WebP, AVIF, TIFF, GIF, BMP, ICO,
  PDF), Resize (largest side, percent, or an exact size fitted or cropped to fill), Remove background, Rotate
  and flip, Remove metadata, Combine into PDF.
- **Video and audio:** Compress (by quality or to a size; a resolution cap; a bitrate for audio), Convert (MP4
  H.264 or HEVC, MOV, WebM, MKV or the sound alone; M4A, MP3, WAV, FLAC, OGG Opus, AIFF), Trim and Make GIF on a
  filmstrip with a range to drag, Rotate, Remove metadata.
- **PDF:** Compress (three presets or a size), Convert to PNG or JPEG pages at 72, 150 or 300 dpi, Rotate pages,
  Remove metadata, Combine. **Anything:** Zip, Rename.

How it is built:

- **Mature tools, fetched on first use.** FFmpeg 9.0.2 (Gyan's essentials build, 115 MB) does video and audio;
  ImageMagick 7.1.2 (portable Q16, 12 MB) and oxipng 10.2 do images; pdfium (bblanchon's Chromium build, 4 MB)
  draws PDF pages and lopdf rewrites PDFs. Each is pinned to an immutable release URL with its size and SHA-256
  (`fetch::Pinned`), downloaded under `%LOCALAPPDATA%\topsy\tools` the first time an action needs it (the
  card says *Getting FFmpeg…*), resumed if cut off, and checked before it is unpacked. Bump a version by
  changing the pin, size and hash together. They run as processes with no window, below normal priority
  (`tools::run`); a cancel kills the process. Nothing of them is bundled or compiled in.
- **HEIC is packed in Rust** (`model/heif`). The portable ImageMagick reads HEIC but has no HEVC encoder, so
  ImageMagick draws a PNG, FFmpeg's x265 encodes it as a one-frame MP4 (the alpha as a second, grey one), and
  topsy moves the coded frames into a HEIF with the colour profile and EXIF. x265 wants even sides in 4:2:0, so
  an odd picture is padded and cropped back with `clap`. FFmpeg's default scaler halves the quality before x265
  starts; the colour conversion asks for accurate rounding and Lanczos chroma.
- **HDR video stays HDR in HEVC,** written 10-bit with its BT.2020 tags. H.264, VP9, GIFs and the filmstrip are
  8-bit SDR, so an HDR source (PQ or HLG, from ffprobe's `color_transfer`) is tone-mapped first: zscale to
  linear light, then the Mobius curve, which keeps the tones SDR can show and rolls off only the highlights.
- **JPEGs turn and strip losslessly, in Rust** (`model/jpeg`), since jpegtran ships for Windows only inside
  installers. A turn that would split an 8x8 block at an edge goes to ImageMagick instead.
- **A photo is turned once.** Its EXIF orientation and the chosen turn compose into one (`model/orientation`)
  and the tag is reset, so no viewer turns the result again.
- **The output's name is reserved before the work** (`create_new`), so two runs never write one file. Compress
  can settle on another format than the name guessed (an opaque PNG becomes a JPEG); the result is renamed to
  match.
- **Remove background runs on this PC:** BiRefNet-lite (a 178 MB model from Hugging Face, fetched once) in pure
  Rust on the CPU, through gemm and rayon.
- **The panel takes the focus,** like the settings window, since it is typed into; the island and the result
  cards never do. The panel's window is transparent outside its rounded corners, with a region a pixel looser
  so they are click-through too (as the floating tray's). A card's picture is cropped to the card before it
  is drawn: GPUI rounds a covering image's overflowing bounds, not the box that clips it.
  The filmstrip's frames are decoded once, in parallel, when a clip is picked; an idle panel renders nothing.

## Notch

The island hangs from the top edge of the screen like a MacBook's notch: square at the top, rounded at the
bottom, with a small concave flare (an "ear") either side where it meets the edge. At rest it is **hidden**:
nothing on screen, nothing to click.

- **Reaching for it.** Resting the cursor on the screen's top row, above where the notch lives, for 150 ms brings
  it down as a 200x32 peek showing what is on the shelf; the delay keeps a sweep across the top from flashing it.
  The mouse hook watches for this (`src/model/edge.rs`), since nothing on screen could: it reports only arriving and
  leaving, so there is still no polling. Dragging something up to that edge opens the tray straight away (and
  brings a floating tray up to the notch). Click the peek to open the tray. It folds back 400 ms after the cursor leaves; the timer asks the
  OS where the cursor is rather than waiting for a hover "leave", which a cursor flicked off the edge doesn't
  always deliver.
- **The floating tray.** Jiggling a drag anywhere opens the tray as a floating panel centred under the pointer, as
  Droppy's *Floating File Tray* does, so letting go there parks the files. It is the same window, moved: rounded
  all round, no ears, with the notch's dynamic glass (black at the top fading to glass) and its rim.
  It opens on the monitor under the pointer, at that monitor's scale, moved only as far as it takes to stay whole
  inside the work area (`src/model/float.rs`). It folds by the jiggle tray's rules (a release without a drop, a
  refused payload such as text, the timer after a drop), shrinking where it is, and the window then goes back to
  the notch. One window rather than two: a second would need its own glass, drop-target wrapper and
  non-activating setup, and could still be on screen with the notch. The top edge and the tray icon open the
  notch tray as before. A jiggle while the tray is already open leaves it where it is.
- **Not over fullscreen apps.** While the foreground window fills the monitor under the cursor (a video, a game, a
  slideshow), the top edge, a drag to it and the jiggle all do nothing, since games drag the mouse about freely.
  It is checked when one of them happens rather than tracked, so it costs nothing in between. A maximized window
  doesn't count (with an auto-hidden taskbar it overhangs the monitor too), nor does the desktop. The tray icon
  still opens the tray.
- **Hidden is a 1 px window with an empty region.** GPUI can't draw into an empty window, and keeping the window
  avoids recreating it (and the activation that brings, see Findings).
- **It is raised whenever it appears.** At launch the island hands the foreground back to the app that had it,
  which puts that app above it if it is topmost too, and other topmost windows can come along while it is hidden.
  The move that shows the notch also puts it at the top of the topmost band, without activating it.
- **Black at the edge, glass below, measured from Droppy.** Sampled from Droppy's own screenshots: solid black
  for the top 62% of the notch, then a straight fade to about 62% black at the bottom edge, so the backdrop's own
  colour shows through darkened, never greyed; ears that reach out about 14% of the height (16 px on the tray);
  and a light rim along the bottom edge, fading out up the sides where the notch is black. The closed notch is
  black all the way down. The ears are GPUI paths with their handles halfway in (softer than a circle, as
  Droppy's are) and are part of the window region.

## Glass

The notch is a real blur of whatever is behind it, in its own shape. The shape follows the spring.

GPUI presents through DirectComposition as the window's *topmost* visual tree. A window can also carry one
non-topmost tree from Windows.UI.Composition, and DWM composes it underneath GPUI's. `src/backdrop.rs` puts a
single visual there: the compositor's host backdrop brush (the blurred desktop behind the window, enabled by
`DWMWA_USE_HOSTBACKDROPBRUSH`) clipped to a rounded rectangle whose radius we choose. Its top corners sit above
the window and are cut off with it, which gives the notch its square top. GPUI draws the tint and rim on top. The blur holds while the window is inactive, which the island always is.

The window-wide alternative, `SetWindowCompositionAttribute` with `ACCENT_ENABLE_ACRYLICBLURBEHIND` (what
GPUI's own `WindowBackgroundAppearance::Blurred` does), blurs the **whole window rectangle** and ignores
`SetWindowRgn` on the Windows 11 build tested (26300); only DWM's fixed ~8 px corner (`DWMWCP_ROUND`) clips it.
It is kept as the fallback where host backdrops are missing (before Windows 11) and as `TOPSY_GLASS=acrylic`;
it can't leave the ears' corners clear, so that notch has no ears and DWM's corners.

Consequences and the GPUI details that had to be got right:

- **The window is the notch.** It is resized and re-centred every frame as the spring moves, and the
  backdrop's clip is reshaped in the same step, so nothing outside the notch draws.
- **The window region shapes clicks.** `SetWindowRgn` (body plus ears) makes the transparent corners
  click-through. It clips drawing too, without anti-aliasing, so its corner is a pixel tighter than the drawn one
  and falls just outside the compositor's smooth edge.
- **The resize must not happen inside `render`.** `SWP_ASYNCWINDOWPOS` only defers across threads; here it is
  synchronous, so `WM_SIZE` reaches GPUI mid-draw, where its resize callback can't update the window and GPUI
  never learns the new size. The move runs in a foreground task (what GPUI's own `Window::resize` does). The
  notch is then drawn at the viewport size GPUI reports, so its edge always matches the window's, one frame
  behind the spring.
- **GPUI creates popups with window style 0**, which Windows treats as overlapped: it enforces a minimum height
  (39 px, taller than the peek) and adds a caption once the titlebar isn't "transparent". With
  `titlebar: None`, GPUI instead insets the client area by an invisible 8 px resize frame. So: pass
  `appears_transparent: false` and replace the style with a bare `WS_POPUP` right after creation.

`scripts/check-glass.ps1` proves the blur rather than eyeballing it. It puts 6 px black/white stripes behind
the open tray (`TOPSY_IDLE=tray`) and compares stripe contrast through its glass, below the black top, with
the contrast beside it, in four modes:

| `TOPSY_GLASS` | What it is | Contrast kept |
|---|---|---|
| `clear` | transparent fill, no blur | 1.00 (control: proves the measurement) |
| `tint` | the same fill, no blur | 0.31 (what the dark fill alone does) |
| *(default)* | compositor backdrop, notch-clipped | **0.04** (13% of what the tint leaves) |
| `acrylic` | window-wide acrylic, the fallback | 0.06 (19%) |

It samples the bottom of the tray, where the fill has faded most. The `tint` control matters: the fill is still
about 70% opaque there, so it cuts stripe contrast by itself, and without the control that would look like blur. A first attempt used `DwmEnableBlurBehindWindow`, which does honour an
explicit blur region but is a weak blur (0.30 against the old, lighter tint's 0.45), with the stripes plainly visible
through it. It was dropped.

## Live-desktop checks (`scripts/`)

These drive the real desktop, so they move your cursor and/or show a window for a few seconds. Keep your hands off
the mouse while one runs: a real click lands among the injected ones and fails it (the `e2e-shelf` "flake" was
exactly that). The checks that click inside the tray or measure it run with `TOPSY_NO_MEDIA`, since whatever is
playing adds a row there.

- `e2e-drag.ps1` — starts a genuine OLE file drag from a fixture window, jiggles it with injected input,
  verifies the floating tray opens under the pointer, releases the drag on it, and checks the drop arrives with the
  right file name. It checks that the window under the drop point is the island's first and aborts otherwise, so a
  drag is never released unless the tray is there to catch it.
- `e2e-shelf.ps1` — seeds a shelf, clicks the peeking notch open, drags a chip onto a separate app (a fixture window,
  `drop-target.ps1`) and checks it received the file and the chip left, cancels a drag with Escape (the chip stays),
  removes a chip with its cross without opening it, opens a folder chip in Explorer, clears the shelf, and finally
  drops a file from another app onto the island to prove it is still a drop target after being dragged out of.
  Escape is held for 250 ms, as the drag loop polls the key.
- `e2e-tray.ps1` — checks the real notification area: the icon is registered with the shell and drawn there, left
  click toggles the shelf, the menu's items work (real keystrokes), *Start with Windows* writes and then clears the
  real Run value, *Show in Send to* creates the shortcut (pointing at the exe) and removes it, *Black notch* saves the look and turns it back, right-clicking the notch opens the menu, and
  Quit exits cleanly and removes the icon. It handles an auto-hidden taskbar, and briefly sets the shell's "show
  this icon" flag for topsy under `HKCU\Control Panel\NotifyIconSettings`; that flag, the Run value and the
  `Look` value are restored when it finishes, and so is any Send to shortcut it found.
- `check-overlay.ps1` — the notch starts hidden (nothing hit-testable), comes down when the cursor rests on the
  top edge, then only the notch is hit-testable, and it folds once the cursor leaves; neither launching nor the
  reveal changes the foreground window. Last, it puts a borderless fullscreen window in front (`fullscreen.ps1`)
  and checks the edge no longer reveals. Run it on a release build (see above).
- `check-glass.ps1` — the blur proof described under [Glass](#glass). Shows a stripe window (`stripes.ps1`)
  at the top of the screen for about 40 s.
- `trace-width.ps1` / `shot.ps1` — sample or screenshot the notch to look at the animation as composited
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
- **`WS_EX_NOACTIVATE` only keeps a click from taking the foreground from another app.** While topsy holds the
  foreground itself (its settings window is in front), a click on the island still made it the active window,
  as GPUI leaves `WM_MOUSEACTIVATE` to `DefWindowProc`. The island's window is subclassed to answer
  `MA_NOACTIVATE`. Likewise the tray icon's menu makes its hidden owner foreground (or the menu won't close) and
  must hand foreground back after, or keys go nowhere until the next click.
- **`SetCursorPos` does not reliably reach `WH_MOUSE_LL`**; test input must be `mouse_event`/`SendInput`.
- Low-level hooks do see button-up and moves during an OLE drag loop.
- **A drag out with Escape needs the key held a moment.** The system's drag loop polls the key state rather than
  being sent the keystroke, because the island never takes focus. A tap shorter than a frame can be missed;
  releasing the button over something that refuses files cancels too.
- The GNU build links and runs (77/77 unit tests, all live checks pass); the binary imports `msvcrt.dll`, no
  VCRUNTIME/MSVCP.

## Measured (release build, GNU toolchain, Windows 11, one 3440x1440 display, idle)

| | |
|---|---|
| Working set | ~70 MB (private ~56 MB) |
| Threads | 16 (15 without the mouse hook), Now Playing's and WinRT's included |
| Binary | 37.1 MB (unstripped; the earlier MSVC build was 10.6 MB) |
| Idle CPU | ~1–2% of one core, with or without the hook or Now Playing (a busy machine; see below) |
| Island re-renders at idle | 0 over 10 s |

Idle CPU swings with machine load: the GNU build once measured ~0.5–0.7%, an earlier MSVC build ~1.5–2%, and
the build before Now Playing, the floating tray, chip actions and the settings window measured the same 0.7–2%
as the build after, back to back on a busy machine. The cause was never found (not our render loop, not the
hook, not the media thread). It is worth a profile, because an always-on app should be closer to 0%.

## Not done / known gaps

- Real jiggle thresholds are untuned (tested only with synthetic input). The shelf takes files only, by choice:
  a jiggled text or link drag still opens the floating tray (the hook can't tell what is being dragged), the drop
  target refuses the payload at once, the tray folds itself away, and further jiggles of that drag are ignored
  until the button is released.
- Multi-monitor and non-100% DPI are handled in code (scale factor, the notch anchors to the monitor it opens on,
  the floating tray opens on the pointer's monitor at its scale) but only tested on one 100% display. A floating
  tray on a monitor of another DPI relies on GPUI's DPI-change handling, then is put back to its own size and
  place; that hand-off is untested. The notch and its edge zone are placed at launch and don't follow a
  display or DPI change until restart.
- Reaching the top edge above the notch reveals it over a maximized app's own controls at the top (tabs, a
  title bar). Fullscreen is only detected as a window covering the monitor, so exclusive-mode games are untested.
  A taskbar docked at the top is not handled.
- Glass: the fill, ears and rim are matched to Droppy's screenshots by sampling them, and compared over a vivid
  blue and a purple backdrop, not side by side with Droppy itself. The host backdrop's blur is heavier than
  Droppy's (whose backdrop stays recognisable through the glass) and its strength can't be set; it is plain blur,
  without acrylic's luminosity layer and noise. No Liquid Glass refraction at the edges. Not verified: Windows 10 (falls back to
  acrylic with square corners), behaviour with "Transparency effects" turned off, and light backdrops.
- Resizing a window every frame is not profiled, and the window trails the spring by a frame. No tearing showed
  in sampled frames, but it was not measured at speed.
- A chip's picture is fetched once, when it is parked; a file edited while on the shelf keeps its old thumbnail.
  Until the picture arrives, or if the shell has none, the chip shows an extension badge.
- The target decides copy, move or link (the default shell drop source offers all three), so a drag to a folder on
  the same drive moves the file, as in Explorer; the chip then leaves the shelf.
- *Compress to ZIP file* shows no progress, and a big folder can take a while before its chip appears. Errors only
  go to stderr. Quitting mid-zip leaves the partial zip behind.
- A file dragged straight into a tray already opened by a click keeps the player on screen until it lands; a
  jiggle on the way turns it into the shelf's drop zone. The drop lands anywhere on the tray either way.
- Now Playing's level bars read the default output device, so a player routed to another output (per-app output
  in Sound settings) shows resting bars.
- Now Playing follows only the system's *current* session (the one Windows picks), with no way to switch between
  several. Its art is fetched on every properties change, not cached by track. Not checked on the desktop yet:
  apps that report art late or not at all, and an app that never answers a button.
- Clipboard history: if another app holds Ctrl+Alt+V the strip can't open (logged), and the hotkey can't be
  changed. Paste sends Ctrl+V, which a terminal that pastes on Ctrl+Shift+V ignores (the entry is still on the
  clipboard). Rich text and HTML are kept as plain text. The cap counts entries, not bytes, so a long history of
  big images kept forever can grow large. The strip shows the newest 100 matches. Tested on one 100% display only.
  Site icons: an SVG-only icon isn't drawn; a site unreachable or busy when first shown is asked again next run, not
  sooner; a name can still re-resolve to a private address between the check and WinHTTP's own lookup; and
  *Clear unpinned* leaves the kept icons, which name the sites copied.
- HUDs: brightness shows only for displays whose driver reports to WMI (a laptop's own panel, not an external
  monitor), and next to Windows' own brightness flyout. Volume keys that a keyboard's own software handles
  without sending the standard key codes don't reach topsy, and with no output device a volume key does nothing.
  The HUD shows over a fullscreen video or game, since the key was taken from Windows. No Caps Lock, keyboard
  backlight or device HUDs yet.
- Transform: ImageMagick's and oxipng's builds link the Visual C++ runtime (`vcruntime140.dll`,
  `vcomp140.dll`), so a PC without it can't run the image actions (FFmpeg and pdfium don't need it). Writing HEIC
  needs FFmpeg too. A video keeps its main picture and its sound: subtitles, data tracks and cover art are
  dropped. An animated GIF or WebP comes out as its first frame.
- No droplets yet. The settings window is a plain dark fill, not Mica.

## License

[MIT](LICENSE). The crates this builds on (GPUI and its dependency tree, 485 packages on Windows) are all
permissively licensed (MIT, Apache-2.0, BSD, Zlib, ISC and similar); a distributed binary should ship their notices. The tools Transform downloads on first use (FFmpeg under the
GPL, ImageMagick, oxipng, pdfium) are separate programs fetched from their own releases, never shipped with topsy.

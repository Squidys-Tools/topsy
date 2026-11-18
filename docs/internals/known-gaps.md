# Not done / known gaps

Keep this honest: remove a gap you close, add one you find and leave.

- **Jiggle.** Real jiggle thresholds are untuned (tested only with synthetic input). The shelf takes files only,
  by choice: a jiggled text or link drag still opens the basket (the hook can't tell what is being dragged), the
  drop target refuses the payload at once, the basket closes itself, and further jiggles of that drag are ignored
  until the button is released.
- **Displays.** Multi-monitor and non-100% DPI are handled in code (scale factor, the notch anchors to the monitor
  it opens on, the basket opens on the pointer's monitor at its scale) but only tested on one 100% display. A
  basket on a monitor of another DPI relies on GPUI's DPI-change handling, then is put back to its own size and
  place; that hand-off is untested. The notch and its edge zone are placed at launch and don't follow a display
  or DPI change until restart.
- **Top edge.** Reaching the top edge above the notch reveals it over a maximized app's own controls at the top
  (tabs, a title bar). Fullscreen is only detected as a window covering the monitor, so exclusive-mode games are
  untested. A taskbar docked at the top is not handled.
- **Glass.** The fill, ears and rim are matched to Droppy's screenshots by sampling them, and compared over a
  vivid blue and a purple backdrop, not side by side with Droppy itself. The live glass rests on undocumented dwmapi
  ordinals, tested on build 26300 only: another build could accept them and draw nothing (one other project reports
  that), and nothing detects it; `TOPSY_GLASS=host` is the way out. No luminosity layer. The lens (the open tray's
  refraction) shows what was behind the notch as it opened unless *Live glass* is on, which costs a few percent
  of GPU for as long as the tray is open, even over a still desktop. The lens is untested on a second monitor and
  at scales other than 100%. Not verified: Windows 10
  (falls back to acrylic with square corners), behaviour with "Transparency effects" turned off, and light
  backdrops.
- **Motion.** Resizing a window every frame is not profiled, and the window trails the spring by a frame. No
  tearing showed in sampled frames, but it was not measured at speed.
- **Chips.** A chip's picture is fetched once, when it is parked; a file edited while on the shelf keeps its old
  thumbnail. Until the picture arrives, or if the shell has none, the chip shows an extension badge. A PDF's chip
  shows its first page only once pdfium has been downloaded (by the Transform panel or *Convert* on a PDF);
  before that it has the shell's icon.
- **Zip.** *Compress to ZIP file* shows no progress, and a big folder can take a while before its chip appears.
  Errors only go to stderr. Quitting mid-zip leaves the partial zip behind.
- **Tray.** A file dragged straight into a tray already opened by a click keeps the player on screen until it
  lands. The drop lands anywhere on the tray.
- **Basket.** Droppy's basket settings are not copied. A hidden basket shows nothing on screen until the next
  jiggle or *Show basket*. The rename field draws no blinking caret (nothing repaints at idle), can't be clicked
  to place the caret, and shows the end of a long name while typing there rather than scrolling. The shelf still
  has no rename: the island never takes focus. After the share sheet closes, the basket or island it was opened
  over keeps the foreground until you click elsewhere.
- **Now Playing.** The level bars read the default output device, so a player routed to another output (per-app
  output in Sound settings) shows resting bars. It follows only the system's *current* session (the one Windows
  picks), with no way to switch between several. Its art is fetched on every properties change, not cached by
  track. Not checked on the desktop yet: apps that report art late or not at all, and an app that never answers
  a button.
- **Clipboard history.** If another app holds Ctrl+Alt+V the strip can't open (logged), and the hotkey can't be
  changed. Paste sends Ctrl+V, which a terminal that pastes on Ctrl+Shift+V ignores (the entry is still on the
  clipboard). Rich text and HTML are kept as plain text. The cap counts entries, not bytes, so a long history of
  big images kept forever can grow large. The strip shows the newest 100 matches. Tested on one 100% display
  only. Site icons: an SVG-only icon isn't drawn; a site unreachable or busy when first shown is asked again next
  run, not sooner; a name can still re-resolve to a private address between the check and WinHTTP's own lookup;
  and *Clear unpinned* leaves the kept icons, which name the sites copied.
- **HUDs.** Brightness shows only for displays whose driver reports to WMI (a laptop's own panel, not an external
  monitor), and next to Windows' own brightness flyout. Volume keys that a keyboard's own software handles
  without sending the standard key codes don't reach topsy, and with no output device a volume key does nothing.
  The HUD shows over a fullscreen video or game, since the key was taken from Windows. No Caps Lock, keyboard
  backlight or device HUDs yet.
- **Live Activities.** Only the Downloads folder is watched, so a download saved elsewhere doesn't show, and a
  file any app writes there as `.part` or `.crdownload` counts as a download. Earbuds' battery is read as they
  connect and once more 2 s later, so one Windows learns after that doesn't show; not checked with real
  Bluetooth devices yet. No timers, calls or coding agents yet.
- **Shelf widgets.** Weather is untested with Windows' location on (only the *location is off* card was seen
  live) and shows no town name. The calendar has no events and its month and day names are English. The stats'
  graphs hold only what was sampled while the tray was open. The Pomodoro's end shows as a Live Activity, so with
  *Live Activities* off only the chime says so; quitting topsy forgets a running timer. The row can't be
  reordered by dragging on the notch, only with the settings window's arrows. With all four on the tray is about
  850 px wide and isn't narrowed for a smaller monitor.
- **Tray pages.** Droppy's third page, a grid of every widget, isn't there. With the acrylic fallback (before
  Windows 11) the gap between the notch and its page capsule is glass too and takes clicks, since that glass
  fills the whole window. A swipe turns the page once it has travelled, rather than following the fingers.
- **Transform.** ImageMagick's and oxipng's builds link the Visual C++ runtime (`vcruntime140.dll`,
  `vcomp140.dll`), so a PC without it can't run the image actions (FFmpeg and pdfium don't need it). Writing HEIC
  needs FFmpeg too. A video keeps its main picture and its sound: subtitles, data tracks and cover art are
  dropped. An animated GIF or WebP comes out as its first frame.
- **Compress PDF** leaves alone the pictures it can't re-encode without changing them: black-and-white scans
  (CCITT, JBIG2), JPEG 2000, CMYK that isn't an Adobe-style JPEG, pictures under 8 bits a component, image
  masks, and pictures with a colour-key mask. A PDF that is mostly text or vector art only has its structure
  packed (about a fifth smaller). A picture drawn through a shading or pattern, or from a content stream topsy
  can't parse, is sized as if it filled the page, so it keeps more resolution than it needs. Only measured on
  PDFs of up to a few hundred pages; a PDF of thousands of pictures runs one ImageMagick per picture.
- **Idle CPU** is ~1–2% of a core on a busy machine, not close to 0%, and the cause was never found; see
  [Measurements](../operations/measurements.md).
- No droplets yet.

# Glass

The notch is a real blur of whatever is behind it, in its own shape. The shape follows the spring.

GPUI presents through DirectComposition as the window's *topmost* visual tree. A window can carry one more,
non-topmost tree, which DWM composes underneath GPUI's; `src/backdrop/` puts the glass there, clipped to a rounded
rectangle whose top corners sit above the window and are cut off with it, which gives the notch its square top. GPUI
draws the tint and rim on top. A second such pane is the page capsule under the open tray (see
[Shelf widgets](widgets.md)). Either kind of glass holds while the window is inactive, which the island always is.
While the tray is open, the lens (below) takes over from the notch's pane.

- **Live (the default).** dwmapi's undocumented shared multi-window visual (ordinals 163 and 164, as VLC 4 uses
  them) is a live copy of every window on the virtual screen except the ones excluded (the island itself), updated
  by DWM. It sits in a DirectComposition target, shifted so the part behind the window lines up, under a saturation
  (1.45x, Notchy's value for Droppy's look) and a Gaussian blur of our choosing (4 logical px). The host backdrop's
  blur is fixed and heavier than Droppy's; this one lets what is behind stay recognisable. Grey Perlin grain, added
centred on zero so it lightens nothing, keeps large flat areas from banding, as on acrylic. The visual must be made for
  a window with live preview turned off, so a cloaked tool window of topsy's own hosts it. Ordinal 164 lost an
  argument before build 20000, so the path is only tried where the Windows 11 host backdrop attribute is accepted. The copy
  spans every monitor, so it is clipped to the notch plus the blur's reach *before* the blur: blurring it whole cost
  a quarter of an integrated GPU whenever anything on screen moved. Clipped, the open tray over content changing at
  30 fps costs no measurable GPU over the desktop alone, as the host backdrop does.
- **Host backdrop (the fallback, `TOPSY_GLASS=host`).** Windows.UI.Composition's host backdrop brush (the desktop
  behind, blurred by the system, enabled by `DWMWA_USE_HOSTBACKDROPBRUSH`) through the same saturation, a Direct2D
  colour matrix the compositor runs. Its blur can be added to but never lightened, and Windows.UI.Composition takes
  only some built-in Direct2D effects: no displacement map, so no refraction.

## The lens: the open tray bends what is behind it

The open tray is Liquid Glass: what is behind it, bent by a soft round bevel toward its edge, blurred as much as
the live glass, with a faint colour split and a lit rim (`src/backdrop/lens/`). No compositor effect displaces
pixels, so this is a pixel shader of our own (HLSL, compiled at startup with `D3DCompile`), drawn with Direct3D 11
into a composition swap chain. The hidden notch and the peek keep the compositor glass; so does the basket. The
floating player has a lens of its own (below).

- **A window of its own, just under the island.** GPUI owns the island's swap chain. The lens is a topmost tool
  window placed directly under the island in z-order, and the island's compositor glass is shrunk to nothing while
  it shows, so GPUI's tint and content draw over the lens. Raising the island (every reshape does) lifts it above
  the lens's slot, so the lens is restacked under it after each move. The lens can't be layered or
  `WS_EX_TRANSPARENT`: DirectComposition content on a layered window doesn't show. Instead its window region is
  the notch's body, which the island covers, and it answers `HTTRANSPARENT`, so it never takes a click.
- **It reads the screen once, as the tray opens.** A GDI copy of the area around the notch, taken before the
  lens shows, becomes a mipmapped texture the shader samples. Nothing runs while the tray stays open, and it
  shows in screenshots and recordings. What is behind does not move while it is open.
- **Live glass** (the setting, or `TOPSY_LENS=live`) follows the screen. A capture of the screen would see the
  lens and bend its own drawing, and hiding topsy from captures would hide the open tray from recordings too.
  Instead a second live copy (the live glass's shared visual) of just the region behind the notch, with the island
  and the lens left out, is shown in a window parked beside every screen and captured there with
  Windows.Graphics.Capture. A live copy is new on every composition whether or not anything behind changed, so
  frames never stop on their own; they are capped at 30 a second, and only while the tray is open. It needs the
  live glass: over the host backdrop the lens stays a snapshot.
- **Starting and stopping the capture pumps messages** on the UI thread, so GPUI can run a frame mid-call and
  call back into the lens. Every call records what it wants, and only one applies it; one made meanwhile is taken
  up when the running one ends.
- **The floating player's lens always follows the screen.** A reading goes stale as soon as a window moves, so
  the player's lens needs the live glass and is the compositor glass without it. It reads the screen once as it
  opens, then starts its capture from a task of its own: starting one takes about 40 ms and pumps messages, and a
  player pulled out of the notch must reach Windows' move loop first. It follows at 24 frames a second while the
  pointer is over it (so while it is dragged) and 5 otherwise. Over a still desktop that costs about 2% of a core
  and 0.6% of the GPU at rest, and 6% and 3% under the pointer; *Live glass* is the open tray's alone.
- **It is a child window of the player.** A child's DirectComposition content shows above its parent's
  non-topmost tree (the compositor glass) and below the topmost one (GPUI's), so it needs no z-order of its own,
  and it moves with the player in the same step, where a window of its own would trail a drag by a frame. It
  answers `HTTRANSPARENT`, which hands clicks to the player. Its reading is the player's largest size plus a
  margin all round: a move inside the margin draws the old reading shifted, and only one past it points the live
  copy elsewhere, at the same size.
- **Its cost is paid at startup.** Creating the device and compiling the shader takes about 100 ms (70 of it the
  compile), done when the notch is made so the first open doesn't stall. The device and the shader are shared
  by both lenses. The device stays: about 24 MB more private memory and two more threads at idle, and no idle CPU.

The window-wide alternative, `SetWindowCompositionAttribute` with `ACCENT_ENABLE_ACRYLICBLURBEHIND` (what
GPUI's own `WindowBackgroundAppearance::Blurred` does), blurs the **whole window rectangle** and ignores
`SetWindowRgn` on the Windows 11 build tested (26300); only DWM's fixed ~8 px corner (`DWMWCP_ROUND`) clips it.
It is kept as the last fallback, where neither compositor glass starts (before Windows 11), and as `TOPSY_GLASS=acrylic`;
it can't leave the ears' corners clear, so that notch has no ears and DWM's corners.

## Consequences, and the GPUI details that had to be got right

- **The window is the notch.** It is resized and re-centred every frame as the spring moves, and the
  backdrop's clip is reshaped in the same step, so nothing outside the notch draws.
- **The window region shapes clicks.** `SetWindowRgn` (body plus ears) makes the transparent corners
  click-through. It clips drawing too, without anti-aliasing, and GDI's round corners sit inside a true arc, so a
  region rounded like the drawn corner cuts its smooth edge into steps. Its corners are rounded 4 px less, leaving
  the edge whole; the cost is a few pixels beside each corner that take clicks. The live glass's clip also needs
  the soft border mode, or DirectComposition draws it aliased.
- **A window Windows moves takes its glass along.** Both glasses are offset by the window's origin to line up
  with what is behind, so a floating window dragged by its caption reshapes its glass at the new origin, or it
  would show the screen from where the drag began. The live glass is placed against the virtual screen, whose
  origin moves when a monitor is added, removed or rearranged, under windows that stay put; its hidden host
  window hears `WM_DISPLAYCHANGE` and sets every pane again, and moves a capturing lens's mirror back beside the
  screens, where a new monitor could otherwise show it.
- **The resize must not happen inside `render`.** `SWP_ASYNCWINDOWPOS` only defers across threads; here it is
  synchronous, so `WM_SIZE` reaches GPUI mid-draw, where its resize callback can't update the window and GPUI
  never learns the new size. The move runs in a foreground task (what GPUI's own `Window::resize` does). The
  notch is then drawn at the viewport size GPUI reports, so its edge always matches the window's, one frame
  behind the spring.
- **GPUI creates popups with window style 0**, which Windows treats as overlapped: it enforces a minimum height
  (39 px, taller than the peek) and adds a caption once the titlebar isn't "transparent". With
  `titlebar: None`, GPUI instead insets the client area by an invisible 8 px resize frame. So: pass
  `appears_transparent: false` and replace the style with a bare `WS_POPUP` right after creation.

## Proving the blur

`scripts/check-glass.ps1` proves the blur rather than eyeballing it. It puts 6 px black/white stripes behind
the open tray (`TOPSY_IDLE=tray`) and compares stripe contrast through its glass, below the black top, with
the contrast beside it, in five modes:

| `TOPSY_GLASS` | What it is | Contrast kept |
|---|---|---|
| `clear` | transparent fill, no blur | 1.00 (control: proves the measurement) |
| `tint` | the same fill, no blur | 0.49 (what the dark fill alone does) |
| *(default)* | the lens, over the live glass | **0.21** (43% of what the tint leaves) |
| `host` | the lens, over the host backdrop | 0.21 (43%) |
| `acrylic` | window-wide acrylic, the fallback | 0.05 (10%) |

It samples the bottom of the tray, where the fill has faded most, so with the lens it measures the lens: its blur
matches the live glass's deviation, and what is left of the stripes there is the bevel gathering them. With
`TOPSY_LENS=off` the live glass keeps 0.14 (28%; the grain is part of it) and the host backdrop 0.04 (8%). The
blur is light enough to leave some of the 6 px stripes, so the check proves there is a blur, not how light it is.
The lens's blur is a Gaussian sampled on a spiral: a grid of taps lined up with the stripes and kept 60% of them.
The `tint` control matters: the fill is still about half opaque there, so it cuts stripe contrast by itself, and
without the control that would look like blur.
A first attempt used `DwmEnableBlurBehindWindow`, which does honour an explicit blur region but is a weak blur
(0.30 against the old, lighter tint's 0.45), with the stripes plainly visible through it. It was dropped.

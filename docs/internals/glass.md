# Glass

The notch is a real blur of whatever is behind it, in its own shape. The shape follows the spring.

GPUI presents through DirectComposition as the window's *topmost* visual tree. A window can also carry one
non-topmost tree from Windows.UI.Composition, and DWM composes it underneath GPUI's. `src/backdrop.rs` puts a
single visual there: the compositor's host backdrop brush (the blurred desktop behind the window, enabled by
`DWMWA_USE_HOSTBACKDROPBRUSH`) clipped to a rounded rectangle whose radius we choose. The brush runs through a
compositor effect first, a colour matrix that saturates it 1.45x (Notchy's value for Droppy's look), so the dark fill
over it keeps the hue of what is behind rather than greying it. The compositor accepts only Direct2D's built-in effects
and not all of them: no displacement map, so no refraction, and the host backdrop's own blur can be added to but never
lightened. Its top corners sit above
the window and are cut off with it, which gives the notch its square top. GPUI draws the tint and rim on top.
The blur holds while the window is inactive, which the island always is.

The window-wide alternative, `SetWindowCompositionAttribute` with `ACCENT_ENABLE_ACRYLICBLURBEHIND` (what
GPUI's own `WindowBackgroundAppearance::Blurred` does), blurs the **whole window rectangle** and ignores
`SetWindowRgn` on the Windows 11 build tested (26300); only DWM's fixed ~8 px corner (`DWMWCP_ROUND`) clips it.
It is kept as the fallback where host backdrops are missing (before Windows 11) and as `TOPSY_GLASS=acrylic`;
it can't leave the ears' corners clear, so that notch has no ears and DWM's corners.

## Consequences, and the GPUI details that had to be got right

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

## Proving the blur

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
about 70% opaque there, so it cuts stripe contrast by itself, and without the control that would look like blur.
A first attempt used `DwmEnableBlurBehindWindow`, which does honour an explicit blur region but is a weak blur
(0.30 against the old, lighter tint's 0.45), with the stripes plainly visible through it. It was dropped.

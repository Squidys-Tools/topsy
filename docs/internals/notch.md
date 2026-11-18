# Notch

The island hangs from the top edge like a MacBook's notch: square at the top, rounded at the bottom, with a
small concave flare (an "ear") either side where it meets the edge. The OS window *is* the notch: it is resized
and re-centred every frame as the spring moves (see [Glass](glass.md)). User-facing behaviour is in
[The notch](../user/notch.md).

- **Reaching for it needs the hook.** Nothing on screen could notice the cursor on the top row above a hidden
  notch, so the mouse hook watches for it (`src/model/edge.rs`). It reports only arriving and leaving, so there is
  still no polling. The 150 ms dwell keeps a sweep across the top from flashing it.
- **Folding asks the OS where the cursor is.** The 400 ms fold timer reads the cursor position rather than
  waiting for a hover "leave", which a cursor flicked off the edge doesn't always deliver.
- **Escape needs a keyboard hook.** The island has no focus to hear Escape, so a low-level keyboard hook
  (`src/win32/escape.rs`) is installed only while a tray opened by a click or the tray icon is open. It takes
  Escape from the app in front unless a mouse button is down (Escape then cancels a drag), a menu is up, or a
  window of topsy's is in front (the Transform panel and the clipboard strip take Escape themselves).
- **Fullscreen is checked, not tracked.** Whether the foreground window fills the monitor under the cursor is
  asked when the edge, a drag to it or a jiggle happens, so it costs nothing in between. Games drag the mouse
  about freely, hence the rule. A maximized window doesn't count (with an auto-hidden taskbar it overhangs the
  monitor too), nor does the desktop.
- **Hidden is a 1 px window with an empty region.** GPUI can't draw into an empty window, and keeping the window
  avoids recreating it and the activation that brings (see [Findings](findings.md)).
- **It is raised whenever it appears.** At launch the island hands the foreground back to the app that had it,
  which puts that app above it if it is topmost too, and other topmost windows can come along while it is hidden.
  The move that shows the notch also puts it at the top of the topmost band, without activating it.
- **Black at the edge, glass below, measured from Droppy.** Sampled from Droppy's own screenshots: solid black
  for the top 62% of the notch, then a straight fade to about 62% black at the bottom edge, so the backdrop's own
  colour shows through darkened, never greyed; ears that reach out about 14% of the height (16 px on the tray);
  and a light rim along the bottom edge, fading out up the sides where the notch is black. The closed notch is
  black all the way down. The ears are GPUI paths with their handles halfway in (softer than a circle, as
  Droppy's are) and are part of the window region.

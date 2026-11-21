# Live-desktop checks (`scripts/`)

These drive the real desktop, so they move your cursor and/or show a window for a few seconds. Run one at a time,
from one worktree at a time, and keep your hands off the mouse while one runs: a real click lands among the
injected ones and fails it (the `e2e-shelf` "flake" was exactly that). The checks that click inside the tray or
measure it run with `TOPSY_NO_MEDIA`, since whatever is playing adds a row there. Judge focus and footprint on a
release build (see [Building](building.md)). Each script states its usage in its header. They share their helpers
(waiting on the log, injected input, starting topsy and stopping it by its own PID) in `scripts/lib/Topsy.psm1`.

- `e2e-drag.ps1`: starts a genuine OLE file drag from a fixture window, jiggles it with injected input,
  verifies the basket opens under the pointer without taking focus, releases the drag on it, and checks the drop
  arrives with the right file name. It checks that the window under the drop point is topsy's first and aborts
  otherwise, so a drag is never released unless the basket is there to catch it. Then it checks the basket is
  still open seconds later, drags its stack onto a separate app (`drop-target.ps1`), and checks the file landed
  there and the emptied basket closed.
- `e2e-textdrag.ps1`: a jiggled drag of text, not files. The basket opens (the hook can't tell), the drop target
  refuses the drag, the empty basket closes at once while the button is still down, a second jiggle doesn't
  reopen it, and nothing lands.
- `e2e-basket.ps1`: seeds a saved basket with a file that no longer exists, checks it comes back without it,
  brings it up with *Show basket* from the notch's menu without taking focus, opens the list from the pill,
  renames a tile with real keystrokes (the field gets the keyboard, the file is renamed on disk, the focus goes
  back, the new name is saved), opens the share sheet for it and checks it asked for the file, then restarts
  the app and checks the renamed file came back.
- `e2e-shelf.ps1`: seeds a shelf, clicks the peeking notch open, drags a chip onto a separate app (a fixture
  window, `drop-target.ps1`) and checks it received the file and the chip left, cancels a drag with Escape (the
  chip stays), removes a chip with its cross without opening it, opens a folder chip in Explorer, clears the
  shelf, and finally drops a file from another app onto the island to prove it is still a drop target after
  being dragged out of. Escape is held for 250 ms, as the drag loop polls the key.
- `e2e-tray.ps1`: checks the real notification area: the icon is registered with the shell and drawn there, left
  click toggles the shelf, the menu's items and its *Options* submenu work (real keystrokes), *Start with Windows* writes and then clears
  the real Run value, *Show in Send to* creates the shortcut (pointing at the exe) and removes it, *Black notch*
  saves the look and turns it back, right-clicking the notch opens the menu, and Quit exits cleanly and removes
  the icon. It handles an auto-hidden taskbar, and briefly sets the shell's "show this icon" flag for topsy under
  `HKCU\Control Panel\NotifyIconSettings`; that flag, the Run value and the `Look` value are restored when it
  finishes, and so is any Send to shortcut it found.
- `check-overlay.ps1`: the notch starts hidden (nothing hit-testable), comes down when the cursor rests on the
  top edge, then only the notch is hit-testable, and it folds once the cursor leaves; neither launching nor the
  reveal changes the foreground window. Last, it puts a borderless fullscreen window in front (`fullscreen.ps1`)
  and checks the edge no longer reveals. Run it on a release build.
- `check-glass.ps1`: the blur proof described in [Glass](../internals/glass.md). Shows a stripe window
  (`stripes.ps1`) at the top of the screen for about 50 s.
- `trace-width.ps1` / `shot.ps1`: sample or screenshot the notch (or, with `shot.ps1 -Rect`, any part of the
  screen, such as the basket) to look at the animation as composited (`trace-width.ps1` needs
  `TOPSY_GLASS=solid`). Captures go in `/shots`, which is gitignored.
- `measure-idle.ps1` / `count-renders.ps1`: idle footprint and re-render count; see
  [Measurements](measurements.md).

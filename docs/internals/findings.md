# Findings: focus and input

What Windows and GPUI actually do about activation and injected input, found by testing. The island must never
take the foreground (AGENTS.md, rule 2 of what we never compromise on), and these are the ways it would.

- **`WindowOptions::focus` is ignored on Windows**; GPUI activates the window when it is created.
  `show: false` is *not* a workaround in gpui 0.2.2: it defers placement until `activate()` and the window is
  never painted. What works: create visible, apply `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` + topmost right away,
  and `SetForegroundWindow` back to whatever had focus. After that nothing activates the island, including
  expanding the tray mid-drag (checked in the e2e). The basket's window is made at launch for the same reason:
  made on the first jiggle, its creation would take the foreground from the app being dragged out of.
- **`WS_EX_NOACTIVATE` only keeps a click from taking the foreground from another app.** While topsy holds the
  foreground itself (its clipboard palette is in front), a click on the island still made it the active window,
  as GPUI leaves `WM_MOUSEACTIVATE` to `DefWindowProc`. The island's window is subclassed to answer
  `MA_NOACTIVATE`.
- **A popup menu needs its owner in front.** `TrackPopupMenu` doesn't close on a click elsewhere unless its owner
  is the foreground window. The chips' and the basket's menus are owned by a hidden window on a thread of its
  own, which is made foreground for the menu and hands foreground back to the window that had it after, or keys
  go nowhere until the next click.
- **Menus open on the button's release.** Opened on the press, the release still goes to the window under it and
  the menu never gets the keyboard: Esc and its access keys do nothing. The basket and the clipboard palette both
  do this.
- **The share sheet only comes up over the foreground window.** Otherwise it reports success and never asks for
  the files, so *Share…* brings the basket (or the island) to the front first.
- **`SetCursorPos` does not reliably reach `WH_MOUSE_LL`**; test input must be `mouse_event`/`SendInput`.
- **Windows that are typed into take focus.** The clipboard palette and the basket's rename field are ordinary
  focusable windows; each remembers the window that was in front and hands focus back when it
  goes. The Transform panel takes focus too but doesn't hand it back. The island and the result cards never take
  focus.

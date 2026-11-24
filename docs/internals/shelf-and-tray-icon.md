# Shelf, tray icon and settings

Decisions behind [the shelf](../user/shelf.md) and [the tray icon and settings](../user/settings.md).

## Shelf

- **Startup never looks at the saved files.** A dead network share would stall launch. Chips come back as `…`
  and are settled off the UI thread, which drops entries whose files are gone. Files moved or deleted while
  parked are checked off the UI thread each time the tray opens.
- **No rename on the shelf.** The island never takes focus, so it can't hold a text field. The basket's rename
  takes focus for the field and hands it back (see [Findings](findings.md)).
- **A chip's picture is the shell's, except for PDFs.** Windows draws a PDF's thumbnail only through a handler
  the PDF app registers; with a browser as the default there is none and the shell returns a generic icon. So
  `icon::pixels` draws the first page with pdfium when it is installed. Parking a file never downloads pdfium;
  only Transform does.
- **Zips are written and extracted on a worker thread**, and what they made is deleted if they fail half-way.
  Links and junctions inside a folder are left out. An entry name is checked by `model::archive::entry_path`
  before anything is written, including the names Windows would trim into `..`.
- **Ctrl and Shift are read with `GetAsyncKeyState` at the click**, not from GPUI's event: the island never has
  the keyboard, so its thread's key state, which GPUI reads, can be stale.
- Opening a chip and its menu pump messages, so they run off the handler; see [GPUI traps](gpui.md).

## Tray icon

- The icon lives on its own thread with a hidden window, re-adds itself when Explorer restarts
  (`TaskbarCreated`), and is removed on Quit.
- **The menu is a glass window of topsy's own**, not a Win32 menu, so it can have the notch's glass. Unlike the
  island it takes the focus while it is up, as a menu does: the click on the icon lets the tray thread make its
  hidden window foreground, the menu takes it from there, and it closes when it loses it. The *Options*
  submenu is a second window that never activates, so the menu keeps the keyboard; while it is being made it
  takes the foreground for a moment, so the menu checks what is in front a frame later before closing. A
  choice that opens a window of topsy's (*Clipboard history*) leaves the foreground with the hidden window, so
  that window may come to the front; any other (*Settings…* too, which only opens the tray) hands it back.
- **One list of settings.** The menu and the tray's settings page are both built from the list in
  `src/settings/mod.rs`, and each reads its value from where it is saved, so a switch in one shows in the other. A new
  toggle is a variant there, an arm in each of its matches, and what the island does about it
  (`Island::on_setting`).
- **Start with Windows and Send to are never created by themselves.** Each is off until turned on. At launch the
  Run value is repaired if the exe has moved, and the Send to shortcut only if the exe it points at is gone.
- **A second launch hands over to the running topsy.** Explorer's Send to starts `topsy.exe` with the chosen
  paths; Start starts it with none; *Uninstall* passes `--uninstall`. Each new process sends its launch
  (`WM_COPYDATA` to a message-only window, `topsy.inbox`) and exits: the running one parks the paths, opens the
  tray, or quits. A plain launch or an uninstall only goes to a topsy running the same exe, so a development
  build and the installed nightly run side by side.
- **Only a nightly registers itself** in Start and Installed apps (`src/install.rs`), rewriting both at every
  launch so they follow the exe. A build without a release stamp never does, so no worktree's build lands in
  Start. Uninstall removes only entries that start its own exe.
- **The settings are a page of the tray, not a window.** Nothing on it is typed into, so it needs no focus, and
  it springs open and folds like the rest of the island. It reads the saved values when it is turned to (not
  every frame, since some are registry reads) and after the menu changes one.

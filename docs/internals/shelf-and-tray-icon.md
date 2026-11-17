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
- **Zips are written on a worker thread** and deleted if they fail half-way. Links and junctions inside a folder
  are left out.
- Opening a chip and its menu pump messages, so they run off the handler; see [GPUI traps](gpui.md).

## Tray icon

- The icon lives on its own thread with a hidden window, re-adds itself when Explorer restarts
  (`TaskbarCreated`), and is removed on Quit.
- **One list of settings.** The menu and the settings window are both built from the list in
  `src/settings.rs`, and each reads its value from where it is saved, so a switch in one shows in the other. A new
  toggle is a variant there, an arm in each of its matches, and what the island does about it
  (`Island::on_setting`).
- **Start with Windows and Send to are never created by themselves.** Each is off until turned on. At launch the
  Run value is repaired if the exe has moved, and the Send to shortcut only if the exe it points at is gone.
- **Send to hands over to a running topsy.** Explorer starts `topsy.exe` with the chosen paths. If topsy is
  already running, the new process sends them (`WM_COPYDATA` to a message-only window, `topsy.inbox`) and exits,
  and the running one parks them and opens the tray.
- The settings window is an ordinary window and takes focus, unlike the island. There is one at a time.

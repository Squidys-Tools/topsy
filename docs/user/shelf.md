# The shelf

The shelf holds files on the notch until you need them. Drop files on the notch and they wait in the tray as
**chips**: the file's thumbnail or icon, as Explorer shows it, and its name. Chips are references, not copies;
the file itself is never touched.

## Using it

- **Add files** by dragging them to the top edge of the screen and dropping them on the notch, from a
  [basket](basket.md) stack, or with Explorer's *Send to* → *topsy* (turn on *Show in Send to* first, see
  [Tray icon and settings](settings.md)).
- **Take a file out** by dragging its chip onto any window, as you would from Explorer. The window you drop on
  decides whether the file is copied or moved, as in Explorer, so a drag to a folder on the same drive moves
  it. A chip leaves the shelf once a drop is accepted.
- **Click a chip** to open the file. The cross on a chip removes it from the shelf. **Clear** empties the
  shelf.
- **Right-click a chip** for *Transform…*, *Share…* (Windows' share sheet), *Show in folder*, *Copy as path*
  (quoted, as Explorer copies it), *Compress to ZIP file* and *Remove from shelf*.

*Compress to ZIP file* writes the zip beside the original, named as Explorer would (`report.zip` for
`report.pdf`; a folder keeps its name and is zipped whole; `report (2).zip` if the name is taken), and parks it
on the shelf when it is done. It never overwrites a file. Links and junctions inside a folder are left out. If
it fails (a locked file, no room), the half-written zip is deleted and no chip appears.

## Good to know

- The shelf is saved to `%LOCALAPPDATA%\topsy\shelf.txt` on every change and comes back on the next start.
  Files moved or deleted since they were parked drop off the shelf.
- There is no rename on the shelf: the notch never takes the keyboard. Rename a file from the
  [basket](basket.md).
- Dropping a chip back on the shelf cancels the drag. Press and hold **Escape** for a moment to cancel a drag
  out.
- The shelf takes files only. Dragged text or a link isn't parked.
- A chip's picture is taken when the file is parked; a file edited while on the shelf keeps its old picture.
  A PDF shows its first page once the PDF viewer Transform uses has been downloaded.
- *Compress to ZIP file* shows no progress, and a big folder can take a while before its chip appears.

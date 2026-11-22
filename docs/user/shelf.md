# The shelf

The shelf holds files on the notch until you need them. Drop files on the notch and they wait in the tray as
**chips**: the file's thumbnail or icon, as Explorer shows it, its name and its size. Chips are references, not copies;
the file itself is never touched.

## Using it

- **Add files** by dragging them to the top edge of the screen and dropping them on the notch, from a
  [basket](basket.md) stack, or with Explorer's *Send to* → *topsy* (turn on *Show in Send to* first, see
  [Tray icon and settings](settings.md)).
- **Take a file out** by dragging its chip onto any window, as you would from Explorer. The window you drop on
  decides whether the file is copied or moved, as in Explorer, so a drag to a folder on the same drive moves
  it. A chip leaves the shelf once a drop is accepted.
- **Click a chip** to open the file. The cross on a chip removes it from the shelf.
- **Pick several chips** with the tick button at the top left: each chip then shows a circle, and a click
  picks it or puts it back. Ctrl-click and Shift-click pick as in Explorer without it. Click the tick button
  again, or an empty part of the shelf, to let them go; folding the tray lets them go too. Dragging a picked
  chip takes all of them out at once, and the right-click menu acts on them all. With chips picked, the
  buttons beside the tick compress them to a zip and show the first in its folder.
- **The buttons at the top right** act on the picked chips, or on the whole shelf when none are picked:
  [Transform](transform.md), send them to the [basket](basket.md) (they leave the shelf and the basket comes up
  under the pointer), and the bin, which takes them off the shelf. The files themselves are never touched.
- **Right-click a chip** for *Transform…*, *Share…* (Windows' share sheet), *Show in folder*, *Copy as path*
  (quoted, as Explorer copies it, one per line for several), *Compress to ZIP file* and *Remove from shelf*. A
  zip also has *Extract here*, and several chips from the same folder have *New folder with selection*.

*Compress to ZIP file* writes the zip beside the original, named as Explorer would (`report.zip` for
`report.pdf`; a folder keeps its name and is zipped whole; `report (2).zip` if the name is taken), and parks it
on the shelf when it is done. Several chips go into one zip named after the first. It never overwrites a file.
Links and junctions inside a folder are left out. If it fails (a locked file, no room), the half-written zip is
deleted and no chip appears.

*Extract here* unpacks a zip into a new folder beside it, named after the zip (`photos (2)` if `photos` is taken),
and parks the folder. A zip with an entry that would land outside that folder is not extracted at all, and a
failed extraction leaves no folder behind.

*New folder with selection* moves the picked files into a new folder beside them (`New folder`, or
`New folder (2)` if that is taken), and their chips become one chip for the folder. If any file can't be moved,
the ones already moved go back and the folder is removed.

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
- *Compress to ZIP file* and *Extract here* show no progress, and a big folder or zip can take a while before
  its chip appears.

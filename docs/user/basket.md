# The basket

Jiggle a drag anywhere on screen (hold the mouse button on a file and move it quickly side to side) and the
basket, a floating file tray after Droppy's, flies in under the pointer to catch it.

## Using it

- **Drop files on it** and they wait there as a stack, the newest in front, over a pill that counts them:
  *1 Image*, *3 Videos*, *2 Items* for a mix. The basket stays open where it is while you switch windows or use
  the notch, until you take the files out.
- **Drag the stack** onto any window to hand over every file at once, as Explorer would, or to the notch to park
  them on the [shelf](shelf.md). Files that land somewhere leave the basket, and it closes when the last one has
  gone.
- **Click the pill** to open the list, a tile per file: drag one out, click it to open it, right-click it for
  *Transform…*, *Share…*, *Rename*, *Show in folder*, *Copy as path*, *Compress to ZIP file* and *Remove from
  basket*. Right-click the stack for the same on every file at once (*Transform…*, *Share all…*, *Copy as
  paths*, *Clear basket*).
- **Move it** by its handle, the strip along the top between the buttons.
- **Close** (✕) empties it; the files themselves are untouched. **Hide** (⌄) puts it away with its files, and
  the next jiggle, or *Show basket* in the tray icon's menu, brings it back.

An empty basket closes by itself once the drag that brought it ends without a drop, or at once if what is being
dragged isn't files (text, say).

## Share

*Share…* is Droppy's AirDrop: Windows' share sheet, with Nearby Sharing, Phone Link, Mail and the other share
targets. The basket comes to the front while the sheet is up, and keeps the foreground after it closes until
you click elsewhere.

## Rename

*Rename* puts a field where the pill is, with the name before the extension selected, as Explorer does. Type to
replace it; arrows, Home and End (with Shift to select), Ctrl+A and Ctrl+C/X/V work. **Enter** renames the file
on disk, **Escape** gives up, and a click elsewhere keeps the new name if it can be used. A name Windows won't
take says why over the field. A file is never renamed over another one. The shelf follows the renamed file.

The field draws no blinking caret, can't be clicked to place the caret, and shows the end of a long name while
typing there.

## Good to know

- The basket is saved to `basket.txt` beside the shelf's file and comes back hidden on the next start. Files
  gone meanwhile are dropped.
- It opens on the monitor under the pointer and is kept whole on that monitor.
- It doesn't open over fullscreen apps (see [The notch](notch.md)).

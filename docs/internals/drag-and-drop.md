# Drag and drop

How the island and the basket take drops and hand files back out, and the traps found the hard way.

## Taking drops

- **GPUI already registers an `IDropTarget` per window**, and the view sees file drops through
  `on_drop::<ExternalPaths>` / `drag_over::<ExternalPaths>`. But a refused drag (anything without `CF_HDROP`,
  like a text selection) gets `DROPEFFECT_NONE` and *no* event, so the app can't hear it through GPUI.
  `win32::watch_drop_refusals` swaps the registered target for one that forwards to GPUI's and reports a refusal
  over a channel. That is how a jiggle-opened basket learns that the drag it opened for (the hook can't tell text
  from files) can never land, and closes itself. The wrapper also re-answers `DragOver` as `DROPEFFECT_NONE` for
  a refused payload: GPUI says `COPY` unconditionally, which would flash a copy cursor and let the drop through.
- Low-level hooks do see button-up and moves during an OLE drag loop, which is what lets the jiggle work on a
  drag that started in another app.

## Dragging out of a window that is itself a drop target

- **The island steps out of drag-and-drop while one of its chips is being dragged.** GPUI registers every window
  as a file drop target, and the drag starts over that window, so the system's drag loop offered the drag to the
  window it came from. GPUI's handler then read the files out of the shell's data object, and that
  intermittently blocked the UI thread for seconds with the loop never returning. `win32::PausedDropTarget` takes
  the registration out for the duration of the drag and puts it back afterwards (GPUI keeps no reference to its
  drop target, so it is read back from the `OleDropTargetInterface` window property and kept alive across the
  revoke). The cost is that dropping a chip back on the shelf is a cancelled drag, which is what you want anyway.
- **The drag loop pumps messages, so it runs from a `cx.spawn` task**, never inside a handler; the same goes
  for opening a chip and its menu. See [GPUI traps](gpui.md).
- **A drag out with Escape needs the key held a moment.** The system's drag loop polls the key state rather than
  being sent the keystroke, because the island never takes focus. A tap shorter than a frame can be missed;
  releasing the button over something that refuses files cancels too.
- **The target decides copy, move or link.** The default shell drop source offers all three, so a drag to a
  folder on the same drive moves the file, as in Explorer; the chip then leaves the shelf.

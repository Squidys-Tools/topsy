# Basket

Decisions behind [the basket](../user/basket.md). Its look is laid out after Droppy's *floating File Tray*, with
sizes and colours sampled from Droppy's feature video; Droppy's basket settings are not copied, since the
feature page and video don't show them.

- **Its own window.** The basket stays up while the notch opens, peeks and folds, so it can't be the island's
  window moved under the pointer. It is a second window made at launch beside the island's, with the same glass
  (`Frame`, floating, to a darker floor than the notch's, as Droppy's is), the same drop-target wrapper and the
  same non-activating styles. Made later, on the first jiggle, its creation would take the foreground from the
  app being dragged out of. Hidden, it is a 1 px window with an empty region, like the island's.
- **Closing on a refused drag.** The hook can't tell what is being dragged, so a text drag opens the basket too.
  The drop-target wrapper reports the refusal and the empty basket closes; further jiggles of that drag leave it
  closed. See [Drag and drop](drag-and-drop.md).
- **The handle is a caption.** The strip between the buttons answers as a caption, so Windows does the moving
  and the basket floats from wherever it is put.
- **Placement.** It opens on the monitor under the pointer, at that monitor's scale, moved only as far as it
  takes to stay whole inside the work area (`src/model/float.rs`); the list grows about its centre and is kept
  whole too.
- **Rename is the one time it takes the keyboard.** It comes to the front for the field and hands focus back to
  the app that had it when the field goes. The field draws no blinking caret, since nothing repaints at idle. A
  rename never replaces another file (a change of case alone is allowed), and a rename made in Transform
  reaches the basket and the shelf too.
- **Share** brings the basket to the front first; see [Findings](findings.md).
- It is saved to `basket.txt` beside the shelf's file on every change; files gone meanwhile are dropped off the
  UI thread, at startup and whenever it opens.

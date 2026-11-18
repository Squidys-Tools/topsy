# GPUI traps

The mistakes that keep biting in topsy's GPUI code. Each one compiles; most abort or hang only at runtime.

- **Inside a closure, use the closure's own `cx`.** `entity.update(cx, |this, cx| ...)`, `cx.listener(...)` and
  `cx.spawn(async move |this, cx| ...)` hand the closure its own context. Reaching for the outer `cx` from inside
  is a second borrow of the app.
- **Updating an entity that is already being updated panics.** A handler on the island that calls back into the
  island through its `Entity` (directly, or through something that does) is a double update. Work on `this`, or
  defer with `cx.spawn` / `cx.defer`.
- **Dropping a `Task` cancels it.** `cx.spawn` and `cx.background_spawn` return a `Task`; awaiting it, calling
  `.detach()`, or storing it in a field keeps it alive. A task stored in a field stops when its owner is dropped,
  which is often what you want (a timer that should die with its view).
- **Never pump messages inside a GPUI handler.** `DoDragDrop`, `ShellExecute`, `TrackPopupMenu`, modal dialogs and
  anything else that runs a message loop deliver the window's messages to GPUI while the app is borrowed, and
  the process aborts. It crashed the app about one run in five in the e2e before it was fixed. Run them from a
  `cx.spawn` task or a thread of their own; a thread also keeps a slow call (a UAC prompt, a cold network path)
  from freezing the island.
- **Never resize or move the window inside `render`.** The move is synchronous, `WM_SIZE` reaches GPUI mid-draw,
  and GPUI never learns the new size. Do it from a foreground task. See [Glass](glass.md).
- **Call `cx.notify()` only when what is drawn changed.** Every notify is a render; topsy should render nothing
  at idle (`scripts/count-renders.ps1`).
- **gpui 0.2.2 ignores `WindowOptions::focus` on Windows** and `show: false` never paints; see
  [Findings](findings.md).

# Droplets

Droppy's droplets are features you switch on and off one by one. topsy's are the same idea, smaller:
a droplet is a type implementing `Droplet` (`src/droplets/mod.rs`) and one line in its `CATALOG`. The shelf
widgets are the first droplets; their own decisions are in [Shelf widgets](widgets.md).

- **Compiled in, not loaded.** Droppy loads signed Swift bundles into its process. Rust has no stable ABI and
  a GPUI element can't cross a DLL boundary, and a WebAssembly runtime is the kind of always-on weight topsy
  leaves out. So every droplet is built into the exe, and adding one is a pull request. A loader for other
  people's droplets is a layer on top of this, if it is ever wanted; nothing here assumes one.
- **The island owns them, it doesn't host views.** A droplet is plain state in `island.droplets`, never a GPUI
  entity of its own, for the reasons in [Features](features.md). All are made when the island starts, on or
  off, and live as long as it does; what the lineup decides is which ones hear `opened` and show their card.
  Switching one on in the settings calls `switched_on`, and off `switched_off`, which must stop whatever it
  started. Nothing is told at launch: a droplet on then starts its work when the tray first opens. Its own tasks can still reach it
  afterwards, so each guards them as it would anyway (the Pomodoro's generation counter).
- **Surfaces are optional methods.** The trait's methods all have defaults; a droplet answers only the moments
  and surfaces it uses. The only surface so far is a card in Home's row (`Droplet::card`). A new kind (a peek
  line, a settings pane of its own) is a new method with a default of nothing, added with the first droplet
  that needs it, and the island's code for that surface asks every droplet that is on.
- **One way in, effects back.** A droplet's listeners and tasks reach it with `update_droplet::<D>`, which finds
  it by type, says whether the row is on screen, and applies the `Effect` it returns. `Effect` is the whole of
  what a droplet may ask of the rest of topsy, so a new ask is a new variant and a compile error until the
  island handles it.
- **The id is the saved name.** The lineup is saved as the droplets' ids (`model::droplets::Lineup`), in the
  registry value the widgets row always used, so a saved row reads the same. An id is never renamed once it
  ships; a saved id this topsy doesn't know is dropped.

## Adding a droplet

1. **Rules** in `crates/model`, with their tests, as for any feature.
2. **A type** in `src/droplets/` implementing `Droplet` and `Default`, and `Card` if it shows in the row. Its
   listeners and tasks go through `update_droplet`; its redraws are booked as in [Features](features.md).
3. **A line in `CATALOG`**: its id, its name, and whether a new lineup has it on. The settings window's switch,
   the arrows that order it, the row's width and `TOPSY_NO_WIDGETS` come with it.
4. **Docs**: a section in `docs/user/`, as for any feature.

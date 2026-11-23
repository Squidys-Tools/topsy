# Features and the island

How a feature (Now Playing, the HUDs, Live Activities, the droplets, and whatever comes next) is wired into the
island, and every place a new one touches.

- **The island is the only view.** `Island` (`src/island/`) is the one GPUI view in the notch's window. Features
  are not entities of their own: a child entity's `notify` would redraw the window behind the island's back, and
  the island's springs, its window resize and its render count all hang off its own render. A feature is a plain
  state type the island owns (`island.droplets`, `island.media`, `island.hud`, `island.activity`).
- **A feature keeps its own logic and never adds methods to `Island`.** Its rendering, timers and handlers are
  methods on its own types, taking what they need as arguments. What only the island knows (whether the feature
  is on screen, the mode) is passed in: `visible` for the droplets, `now_playing::Shown` for Now Playing.
  `scripts/check-structure.ps1` fails any `impl Island` outside `src/island/` and `src/app.rs`.
- **One entry point per feature, and effects back.** Listeners and tasks a feature creates still run on the
  island (they need its `Context`), so each goes through the feature's entry point in
  `src/island/tray_view.rs` (`update_droplets`, `update_media`, `update_hud`, `update_activities`). The entry
  point works out what the feature may need to know, hands it its own state, and applies the small `Effect`
  enum it returns: clear the press, refit the tray, show a Live Activity. The `match` names every variant, so a
  new effect is a compile error until the island handles it. A listener that changes only the feature's own
  state may reach it through its field directly.

## Adding a feature

A small feature that is switched on and off by itself and shows as a card on Home is a
[droplet](droplets.md), which has a shorter list. Otherwise, walk every entry; say in the PR which applied.

1. **Rules** in `crates/model`, with their tests (`cargo test -p topsy-model`). No clock reads: take `now`.
2. **Glue** for the Win32 or WinRT source on a thread of its own, sending plain events over an `async_channel`
   (`app::forward`), in `src/win32/` or the feature's folder.
3. **State and view** in the feature's own module, with its entry point and `Effect` in `tray_view.rs`.
4. **Room on screen.** A section of the tray is a field of `TrayCause::sections` and a term in `size_for`; a
   peek is a branch of `peek_content`.
5. **Redraws.** A clock or a moving bar books its next redraw from its render with a `Booking`
   (`src/booking.rs`), which keeps one timer however often the render runs, and redraws only while the
   feature shows. Check the idle with `scripts/count-renders.ps1`.
6. **A switch to turn it off.** A `Setting` variant (`src/settings/mod.rs`) reaches the tray icon's menu and the
   settings window; a `TOPSY_NO_*` field in `src/config.rs` and a row in
   [Running](../operations/running.md), and the live checks that click the tray turn it off.
7. **Docs**: a section in `docs/user/`, and an internals page only for a decision the code can't show.

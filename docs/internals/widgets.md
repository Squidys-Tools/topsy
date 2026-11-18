# Shelf widgets

Decisions behind [Shelf widgets](../user/widgets.md). The row lives in `src/widgets/`, its rules in
`src/model/` (`widgets`, `calendar`, `pomodoro`, `stats`, `weather`).

- **Only a tray opened to look shows them.** `TrayCause::sections` decides: a drag or a drop is about the shelf,
  so the row stays out of its way, like Now Playing. The tray widens to the row, so its width now comes from
  `size_for` as its height does, and `fit_tray` refits both.
- **Nothing runs off screen.** A card books its next redraw (the stats' next sample, the countdown's next
  second) from its own render, behind a `Cell` flag, and the callback redraws only while `widgets_visible()`.
  The weather fetches from its render when one is due, so a retry waits for the next look. Folded, the row
  costs nothing; opened again, its render books the next round. The one exception is the Pomodoro's end: a
  single timer for the moment it is due, cancelled by a generation counter, so the chime and the passing
  activity come even with the tray folded. Switching the Pomodoro off stops it.
- **Weather has no key and no town.** Open-Meteo needs no account, so there is nothing to ship or leak, and it
  has no reverse geocoding, so the card says *My location*. The position comes from Windows' Geolocator on a
  thread of its own (it blocks), rounded to two decimals before it leaves the PC. Windows refusing it is told
  apart from it failing, so the card can send you to the setting that fixes it.
- **The lineup is one registry string**, read when the island starts and written by the settings window, which
  hands the island the new lineup (`settings_window::Change::Widgets`) rather than having it read the registry
  again.

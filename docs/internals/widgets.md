# Shelf widgets

Decisions behind [Shelf widgets](../user/widgets.md). The row lives in `src/widgets/`, its rules in
`src/model/` (`widgets`, `calendar`, `pomodoro`, `stats`, `weather`).

- **They live on Home, a page of their own.** A tray opened to look, with something for Home, has two pages,
  Home and the shelf, as Droppy's has (`src/pager.rs`, rules in `model::pager`; `TrayCause::sections` says what
  each page shows). A drag or a drop is about the shelf, so it shows the shelf alone, with no pages. Each page has
  its own size, so the tray's width comes from `size_for` as its height does, and `retarget` springs both.
- **The page capsule hangs below the notch, outside it.** The window grows by a strip under the notch for it,
  sprung out from under the notch and back, and its region and compositor glass take a second rounded pane
  (`win32::Capsule`, `Backdrop::capsule`). The window takes a new size a frame after it is asked for, so the
  frame records which strip each window height was made with and splits the drawn frame by the size it has.
- **A swipe turns one page.** Windows delivers a touchpad's two-finger swipe as horizontal wheel messages, as it
  does a tilt wheel and Shift with the wheel, so GPUI sees scroll deltas with no gesture phases. `model::pager`'s
  `Swipe` turns once a sideways scroll travels 30 px and ignores the rest until the scrolling pauses for 250 ms.
  The chips take the swipe while they can still scroll that way. The vertical wheel never turns the page.
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

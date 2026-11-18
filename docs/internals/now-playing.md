# Now Playing

Decisions behind [Now Playing](../user/now-playing.md). A tray opened for a drag or by a drop leaves the player
out (`TrayCause::sections`).

- **Event-driven only.** `src/now_playing/session.rs` subscribes to `CurrentSessionChanged`, `MediaPropertiesChanged`,
  `PlaybackInfoChanged` and `TimelinePropertiesChanged`. WinRT raises them on thread-pool threads, whose handlers
  only wake one `topsy-media` thread; it reads the session (and decodes the art, scaled down by WIC) and sends the
  result to the UI thread over a channel. Between events nothing runs (but for the progress bar and the level
  bars while they are on screen, below), and a change while the notch is hidden doesn't render.
- **The progress bar** is carried forward from the app's last report of its position, which apps send rarely
  and not always on pause, so `model::media` holds the position where it got to when playback stops. While the
  player is open and the track playing, one redraw is booked for each new second; anything else books none.
- **The level bars** move with the default output device's peak meter (`IAudioMeterInformation`, what the
  volume flyout draws), so they follow everything playing, not only the session shown; capturing the app's own
  audio would need a loopback stream, far heavier. While the peek or the player shows them and the track plays,
  the meter is read about 12 times a second (each read is a whole redraw of the island, a few ms of CPU; measured
  at about 2.5–3% of a core on top of idle), and reopened each second to follow a change of output device.
  Otherwise it is let go and nothing is booked. `model::levels` gives the bars an automatic gain (a quiet track
  fills them as a loud one does) and ripples each beat out from the middle. They take a colour averaged from the
  art's vivid pixels, lifted to read on black, or white when the art has none.
- **A long title** scrolls side to side in the player (`model::marquee`): it rests two seconds at each end and
  glides between at 30 px/s, read off the title line's own scroll extent, so no text is measured. Only a glide
  books frames; a rest books one wake for its end, and a tray that closes books nothing more.
- **Buttons** go to the same thread, which calls the session's `Try*Async` without waiting; the icon changes
  when the app reports its new state.

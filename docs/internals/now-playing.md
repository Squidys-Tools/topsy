# Now Playing

Decisions behind [Now Playing](../user/now-playing.md). A tray opened for a drag or by a drop leaves the player
out (`TrayCause::sections`).

- **Event-driven only.** `src/now_playing/session.rs` subscribes to `SessionsChanged`, `CurrentSessionChanged` and
  every session's `PlaybackInfoChanged` (`model::sessions` picks the one shown from them), and the shown session's
  `MediaPropertiesChanged` and `TimelinePropertiesChanged`. WinRT raises them on thread-pool threads, whose handlers
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

## Floating player

- **Its own window, made at launch**, hidden as a 1 px window with an empty region, for the basket's reason: made
  later, its creation would take the foreground. `TOPSY_NO_MEDIA` makes none.
- **Its own copy of what is playing.** `Media::apply` hands every update from the media thread to the player
  too, art included (`Update` is cheap to clone; the art is shared). The island never reads the player's entity
  in its render: GPUI redraws every window that read an entity whenever that entity notifies, so the island would
  redraw with each of the player's frames. What the tray needs (whether the player is out) is a flag the two
  share (`floating::Handle`).
- **Pulling it out** hands the press to Windows' move loop: once the tray player has been dragged a few pixels,
  the floating player is placed at its full size with its art under the pointer, then the island lets go of the
  mouse and the player is posted `WM_NCLBUTTONDOWN` on its caption (`win32::grab`). The move loop keeps the
  pointer's offset from where the window was when it started, so the window must already be in place: it is not
  sprung in, and the post is queued after the move its render asked for. The loop pumps messages, so it is
  posted, never sent from a handler.
- **Cost on screen.** It may stay up all day, so it redraws less than the notch: the level bars move only while
  the pointer is over it (and its hover is checked against the real pointer each frame, so a lost "leave" can't
  keep the meter open); Mini has no clock and books nothing while a song plays; Player and Expanded redraw once
  a second for the clock. The title is cut short rather than scrolled.
- **The cover** is decoded once per track at a second, larger size beside the notch's art (the same thumbnail,
  two WIC transforms): the notch's small art would blur at the cover's size, and the cover is more texture
  than the notch needs.
- **Saved** as `HKCU\Software\topsy\Player` (`x,y,size,open|closed`, `model::player`) when it opens, closes,
  changes size, or stays put after a move. A run with `TOPSY_STATE` or `TOPSY_PLAYER` set neither reads nor
  writes it, so a live check never meets the developer's own player.

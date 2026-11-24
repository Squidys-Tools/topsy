# Shelf widgets

When you open the tray to look (click the notch or the tray icon), a row of small widgets sits on its Home page,
under [Now Playing](now-playing.md), after Droppy's. The [shelf](shelf.md) is the next page: switch with the
capsule under the notch or a sideways swipe (see [The notch](notch.md)).

- **Calendar**: today, its week number and the month. The arrows look at other months; click the date to come
  back to this one.
- **Pomodoro**: a focus or break timer. Pick *Focus* or *Break*, set the minutes with − and + while it is
  stopped, then play. When the time is up you hear a chime and the notch says so for a moment, even with the
  tray folded away; the timer is then ready for the other one.
- **System stats**: how busy the processor is and how much memory is in use, each with a graph of the last
  minute or so.
- **Weather**: the temperature and conditions where you are, today's high and low, and the next few hours.
- **Audio**: a volume slider for every app playing sound, after Droppy's Audio Control. Drag an app's slider to
  set its volume, or click the speaker beside it to mute it; moving a muted app's slider unmutes it. It is the
  same volume as in Windows' volume mixer, so a change in either shows in both.

A drag or a drop opens the tray on the shelf; the capsule turns back to the widgets.

## Choosing them

Under Widgets on the settings page (see [Tray icon and settings](settings.md)), click a widget's dashed **+** pill
to add it, click one that shows to take it out, and drag them into the order you want, left to right. The tray widens to fit them. Calendar, Pomodoro and System stats are on
to begin with; turn every one off (with nothing playing) to get the plain tray back, with no pages. Saved in `HKCU\Software\topsy\Widgets`.

## Good to know

- Widgets only do anything while you are looking at them: the stats are sampled once a second and the weather is
  fetched while the tray is open, never in the background. The graphs start where you last left them.
- **Audio** lists the apps with sound open on your default output device, one row per app however many windows
  or tabs it plays from; Windows' own system sounds are left out. It is off until you turn it on.
- **Weather** asks Windows where you are, so it is off until you turn it on. Windows needs *Location services*
  and *Let desktop apps access your location* on (Settings › Privacy & security › Location); if they are off the
  widget says so and clicking it opens that page. It shows the weather for your position, without a town name.
  Forecasts come from [Open-Meteo](https://open-meteo.com) (data CC BY 4.0), at most every 30 minutes,
  sending your position rounded to about a kilometre. Fahrenheit or Celsius follows your region's measurement
  system.
- The Pomodoro's chime and message come from a timer set for the end of the session, so nothing ticks while the
  tray is folded. Quitting topsy stops the timer.

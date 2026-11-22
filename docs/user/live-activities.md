# Live Activities

While something is going on, the notch stays down a little wider and shows it, after Droppy's:

- **A browser download**, with its name and the size it has reached, then a green tick and *Done*. It stays for
  as long as the download runs, or a minute after it stops growing (paused or failed), and goes 3 s after it
  finishes. Clicking a finished download shows it in Explorer.
- **Bluetooth headphones or a speaker** connecting: a card with its shape, its name and its battery where
  Windows knows it (AirPods and Beats show each bud and the case), which shrinks to a row after 3 s. Leaving shows for 3 s too, and a connected one comes back
  when its battery drops below 20% and 10%.
- **A [Pomodoro](widgets.md) session ending**, with what comes next, for 3 s.

Two show side by side, newest first. Clicking anything but a finished download opens the tray as usual, and an
open tray folds back to the activity. A HUD takes over the notch for its moment, and a fullscreen app in front
keeps it hidden. The *Live Activities* setting turns it all off (see [Tray icon and settings](settings.md)).

## Good to know

- Only the Downloads folder is watched, so a download saved elsewhere doesn't show. There is no progress ring,
  because nothing says how big a download will be.
- A download already running when topsy starts shows once it next grows.

## Your headset

topsy knows popular headsets by model and draws each as its shape: earbuds with or without stems, headphones
or a speaker. One it doesn't know gets the shape its name suggests. To have yours recognised, connect it and
choose *Add my headset…* in the tray icon's menu: if you have Claude Code, Codex or Antigravity installed, it
opens one to add your headset to topsy and send the change for you, asking before anything leaves your
machine. Otherwise it opens a GitHub issue with what topsy saw of it, filled in for you to check and send.

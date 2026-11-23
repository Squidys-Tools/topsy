# Live Activities

Decisions behind [Live Activities](../user/live-activities.md). Nothing polls.

- **Downloads** are told apart from the Downloads folder's changes alone (`ReadDirectoryChangesW` on a thread
  that blocks in it): browsers write to a partial file (`.crdownload` for Chrome and Edge, `.part` for Firefox)
  and rename it when it's done, or delete it if cancelled. Nothing says how big a download will be, so there's no
  ring filling up, only the size so far, read at most once a second while it grows. A paused or failed download
  keeps its partial file, so it never visibly ends; it goes a minute after it last grew.
- **Bluetooth audio** comes from `IMMNotificationClient`: an output endpoint turning active or going away. Its
  device container is looked up in Configuration Manager to tell Bluetooth from a monitor's speakers, and for
  the name and battery; a device's several endpoints count once. While it stays connected, a `DeviceWatcher` on
  its container, asking for the battery property, wakes the thread whenever a node's battery changes, including
  the late first reading Windows often gets a few seconds after the connection. A change while the activity
  shows updates it in place (its 3 s start again); after that, only falling below 20% or 10% brings it back.
- **A headset's model** comes from the product ids in its profile nodes' instance ids, read with the rest from
  Configuration Manager, and is looked up in `devices/catalog.toml`, compiled in; the name is only a fallback,
  since users rename their devices. Connecting opens with a card that has the notch to itself for 3 s, then
  shrinks to the row for its 3 s; a low battery brings back only the row. The card plays its picture's turntable
  once (1.4 s, its redraws booked about 60 a second) and then holds the first view, so nothing repaints after. See
  [Headsets and the device catalog](../operations/devices.md).
- **AirPods and Beats** report each bud and the case only in Apple's Bluetooth LE broadcasts (manufacturer
  data, proximity pairing); Windows gives one battery at most. A passive scan listens for 8 s as an Apple
  headset connects and whenever Windows' battery for it moves, then stops: it is radio time, so it never runs
  on its own. Strangers' AirPods broadcast too, so a scan takes only its model's, at -70 dBm or nearer, and
  keeps the nearest. Once the buds have spoken, Windows' single number only starts another scan.
- **The notch rests in the peek** while an activity lasts: the folds that went to the island's rest go to
  `resting()` instead, and a drag up to the edge opens the tray from that peek as it does from hidden.

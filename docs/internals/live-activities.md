# Live Activities

Decisions behind [Live Activities](../user/live-activities.md). Nothing polls.

- **Downloads** are told apart from the Downloads folder's changes alone (`ReadDirectoryChangesW` on a thread
  that blocks in it): browsers write to a partial file (`.crdownload` for Chrome and Edge, `.part` for Firefox)
  and rename it when it's done, or delete it if cancelled. Nothing says how big a download will be, so there's no
  ring filling up, only the size so far, read at most once a second while it grows. A paused or failed download
  keeps its partial file, so it never visibly ends; it goes a minute after it last grew.
- **Bluetooth audio** comes from `IMMNotificationClient`: an output endpoint turning active or going away. Its
  device container is looked up in Configuration Manager to tell Bluetooth from a monitor's speakers, and for
  the name and battery; a device's several endpoints count once. Windows often learns the battery a few seconds
  after the connection, so a device that had none is asked once more 2 s later, inside its 3 s, and the
  activity updates in place (its 3 s start again).
- **The notch rests in the peek** while an activity lasts: the folds that went to the island's rest go to
  `resting()` instead, and a drag up to the edge opens the tray from that peek as it does from hidden.

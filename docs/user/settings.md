# Tray icon and settings

## Tray icon

topsy's icon in the notification area is a small topsy-turvy notch. **Left-click** it to open or close the
shelf. **Right-click** it, or right-click the notch itself, for the menu: *Show shelf*, *Clear shelf*, *Show
basket*, *Show floating player* (or *Hide* while it is out), *Clipboard history*, *Options* (a submenu with each
setting below, ticked when on), *Settings…*, *Add my headset…* (see
[Live Activities](live-activities.md#your-headset)), *Restart to update* when an update is ready, and *Quit
topsy*. The arrow keys and Enter work in it too, and Escape closes it.

On Windows 11 a new icon starts in the overflow flyout (the ^ arrow); drag it onto the taskbar to keep it
visible.

## Settings page

The settings live in the notch, on the tray's third page: click the gear in the capsule under the open tray, or
choose *Settings…* in the menu. Its sections run down the left: **Notch** and **General** hold the switches below,
**Widgets** which [shelf widgets](widgets.md) show and in what order, **Clipboard** how long
[clipboard history](clipboard-history.md) is kept, and **About** the version, an update waiting (a dot marks the
section) and links to the source and this help. *Quit topsy* is under General. A switch changed in the menu shows
on the page, and the other way round.

## Settings

- **Black notch** draws the notch plain black instead of glass. Saved in `HKCU\Software\topsy\Look`.
- **Live glass** keeps what the open tray bends up to date as the screen behind it changes. Without it the glass
  shows what was there when the tray opened. It uses a little graphics power while the tray is open. The floating
  player's glass always follows the screen, more slowly while the pointer is away from it. Saved in
  `HKCU\Software\topsy\Lens`.
- **Start with Windows** starts topsy when you sign in. It is off until you turn it on, needs no admin rights,
  and writes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\topsy`. If you move `topsy.exe`, the entry
  follows it the next time you start it.
- **Show in Send to** adds topsy to Explorer's *Send to* menu (a `topsy.lnk` in
  `%APPDATA%\Microsoft\Windows\SendTo`). Sending files to topsy parks them on the [shelf](shelf.md) and opens
  the tray, whether topsy is running or not. Off until you turn it on.
- **Clipboard history** and how long it is kept; see [Clipboard history](clipboard-history.md).
- **Volume HUD** lets topsy take the volume keys; see [HUDs](huds.md).
- **Live Activities** shows downloads and Bluetooth audio in the notch; see [Live Activities](live-activities.md).
- **Check for updates** keeps a nightly build up to date; see below. On until you turn it off. Saved in
  `HKCU\Software\topsy\Updates`.

## Updates

A nightly build looks for a newer nightly when it starts and every 4 hours, and downloads it quietly into
`%LOCALAPPDATA%\topsy\update`. When one is ready the notch shows *Update ready* for a few seconds; click it for
the settings page's About, then *Restart to update*. The tray icon's menu has *Restart to update* too. topsy swaps
its own `topsy.exe` and starts again; *Start with Windows* keeps working. An update you don't restart into is
installed the next time topsy starts.

Only nightlies update. A build from source or from CI never does.

## Start and uninstalling

A nightly puts itself in Start (a `topsy.lnk` in `%APPDATA%\Microsoft\Windows\Start Menu\Programs`) and in
*Settings > Apps > Installed apps* the first time it runs, so Start's search and launchers such as Raycast find
it. Starting it again while it runs opens the shelf. If you move `topsy.exe`, both follow it the next time you
start it.

*Uninstall* in Installed apps quits topsy, removes it from Start, startup and *Send to*, and deletes `topsy.exe`.
Your shelf, clipboard history and settings stay in `%LOCALAPPDATA%\topsy` and `HKCU\Software\topsy`; delete
them by hand if you want them gone.

## Quitting

*Quit topsy* in the menu or *Quit* on the settings page quits and removes the icon.

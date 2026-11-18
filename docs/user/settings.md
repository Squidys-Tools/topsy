# Tray icon and settings

## Tray icon

topsy's icon in the notification area is a small topsy-turvy notch. **Left-click** it to open or close the
shelf. **Right-click** it, or right-click the notch itself, for the menu: *Show shelf*, *Clear shelf (N)*,
*Show basket (N)*, an item for each setting below (checked when on), *Settings…* and *Quit topsy*.

On Windows 11 a new icon starts in the overflow flyout (the ^ arrow); drag it onto the taskbar to keep it
visible.

## Settings window

*Settings…* opens *topsy settings*, a small window with a switch for each setting, how long
[clipboard history](clipboard-history.md) is kept, which [shelf widgets](widgets.md) show and in what order, the
version and *Quit topsy*. Asking again brings the open
one to the front. Closing it leaves topsy running. A switch changed in the menu shows in the window, and the
other way round.

## Settings

- **Black notch** draws the notch plain black instead of glass. Saved in `HKCU\Software\topsy\Look`.
- **Start with Windows** starts topsy when you sign in. It is off until you turn it on, needs no admin rights,
  and writes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\topsy`. If you move `topsy.exe`, the entry
  follows it the next time you start it.
- **Show in Send to** adds topsy to Explorer's *Send to* menu (a `topsy.lnk` in
  `%APPDATA%\Microsoft\Windows\SendTo`). Sending files to topsy parks them on the [shelf](shelf.md) and opens
  the tray, whether topsy is running or not. Off until you turn it on.
- **Clipboard history** and how long it is kept; see [Clipboard history](clipboard-history.md).
- **Volume HUD** lets topsy take the volume keys; see [HUDs](huds.md).
- **Live Activities** shows downloads and Bluetooth audio in the notch; see [Live Activities](live-activities.md).

## Quitting

*Quit topsy* in the menu or the settings window quits and removes the icon.

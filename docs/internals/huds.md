# HUDs

Decisions behind [the HUDs](../user/huds.md). None of them polls.

- **topsy takes the volume keys** (the *Volume HUD* setting): a low-level keyboard hook on a thread of its own
  swallows VolumeUp, VolumeDown and Mute, so Windows never shows its own volume flyout, and hands them to a thread
  that steps the default output device by its own step (`IAudioEndpointVolume`), as Windows does; up and down
  unmute. Every other key passes straight through. Switched off, the hook is removed and the keys are Windows'
  again. Windows' flyout can't be hidden any other way: it is the shell's, not an app's.
- **Brightness** comes from WMI's `WmiMonitorBrightnessEvent`, which the display driver raises on each change;
  a thread blocks on it. Windows' own brightness flyout still shows: the keys are the laptop's firmware's.
- **Battery** changes arrive as power-setting callbacks (`PowerSettingRegisterNotification`). A machine without
  a battery registers nothing.

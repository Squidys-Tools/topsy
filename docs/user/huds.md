# HUDs

When the volume, the brightness or the battery changes, the notch drops down a little wider and shows it: an
icon and a bar lit up to the level, or for the battery what happened, the charge and a battery cell. It folds
away 1.5 seconds after the last change, later if the cursor is on it. Over an open tray it shows as a pill
across the top instead.

The battery shows when it is plugged in, when it falls below 20% and 10% (or is unplugged below 20%), and when it
is full on the charger.

## Volume

With the *Volume HUD* setting on (the default), topsy takes the volume keys from Windows, so Windows' own volume
flyout no longer shows, and steps the volume itself, as Windows does. Turn *Volume HUD* off in the settings
window to give the keys back to Windows (see [Tray icon and settings](settings.md)).

## Good to know

- Brightness shows only for displays that report it to Windows (a laptop's own panel, usually not an external
  monitor), and Windows' own brightness flyout still shows beside it.
- Volume keys that a keyboard's own software handles without sending the standard keys don't reach topsy.
- The volume HUD shows over a fullscreen video or game.
- A PC without a battery shows no battery HUD.

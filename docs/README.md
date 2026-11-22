# topsy docs

## Using topsy

- [The notch](./user/notch.md): bringing it down, the peek and the tray
- [The shelf](./user/shelf.md): parking files on the notch
- [The basket](./user/basket.md): jiggle a drag for a floating file tray
- [Now Playing](./user/now-playing.md)
- [Clipboard history](./user/clipboard-history.md)
- [HUDs](./user/huds.md): volume, brightness and battery
- [Live Activities](./user/live-activities.md): downloads and Bluetooth audio
- [Shelf widgets](./user/widgets.md): calendar, Pomodoro, system stats and weather
- [Transform](./user/transform.md): compress, convert, resize, trim
- [Tray icon and settings](./user/settings.md): the menu, start with Windows, Send to, updates

---

## Working on topsy

Start with [AGENTS.md](../AGENTS.md), then [Building](./operations/building.md).

Internal notes keep architectural decisions, constraints across components, and traps the source alone does
not explain. Most code changes need no internal doc change. Follow the
[documentation rules](../AGENTS.md#documentation) before adding one.

- [GPUI traps](./internals/gpui.md)
- [Features and the island](./internals/features.md): how a feature is wired in, and adding one
- [Findings: focus and input](./internals/findings.md)
- [Notch](./internals/notch.md)
- [Glass](./internals/glass.md)
- [Drag and drop](./internals/drag-and-drop.md)
- [Shelf, tray icon and settings](./internals/shelf-and-tray-icon.md)
- [Basket](./internals/basket.md)
- [Now Playing](./internals/now-playing.md)
- [Clipboard history](./internals/clipboard-history.md)
- [HUDs](./internals/huds.md)
- [Live Activities](./internals/live-activities.md)
- [Shelf widgets](./internals/widgets.md)
- [Transform](./internals/transform.md)
- [GNU toolchain and build.rs](./internals/toolchain.md)
- [Not done / known gaps](./internals/known-gaps.md)

### Runbooks

- [Building](./operations/building.md)
- [Running with test switches](./operations/running.md)
- [Headsets and the device catalog](./operations/devices.md): adding a headset, `cargo devices`
- [Live-desktop checks](./operations/live-checks.md)
- [CI, the artifact and nightlies](./operations/ci.md)
- [Measurements](./operations/measurements.md)

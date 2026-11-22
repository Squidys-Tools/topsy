# Headsets and the device catalog

topsy recognises a headset by model through [`devices/catalog.toml`](../../devices/catalog.toml): each entry
gives a model its name, its shape and the rules that match it. A device no entry matches still shows, drawn as
the shape its name and Bluetooth class suggest. The rules are in `crates/model/src/headset/`.

## Adding a headset

The usual way is topsy's own: the tray icon's menu has *Add my headset…*, and so does `topsy.exe
--add-headset`. It writes `%LOCALAPPDATA%\topsy\add-headset\context.json` (the user's Bluetooth audio devices:
names, product ids, class, battery and device ids, with Bluetooth addresses blanked) beside
[`PLAYBOOK.md`](../../.github/devices/PLAYBOOK.md), then opens a console running the first of `claude`,
`codex` or `agy` on `PATH`, told to follow the playbook. Without one, it opens a prefilled
[*Add a headset*](../../.github/ISSUE_TEMPLATE/add-headset.yml) issue instead. The playbook is compiled into
topsy and tells the agent to prefer the copy on `main`, so improving it helps old installs too.

By hand, with a `context.json` from that menu:

```
cargo devices match <context.json>     which entry each device matches, and why
cargo devices add <context.json> --device "<name>" --slug <slug> [--name "<model>"] [--form <form>]
cargo devices check                     the catalog's checks, as the tests run them
```

`add` puts the device's product ids on the entry with that slug, or makes a new entry, which needs `--name`.
`cargo devices` is its own crate (`crates/devices`), depending only on `crates/model`, so it builds in seconds
without GPUI.

Then preview it: `TOPSY_FAKE_DEVICE=<slug>:80` (or `:L80,R75,C40` for earbuds that report each bud) shows the
entry connecting at startup, card and all, on a build with the new catalog ([Running](running.md)).

## What makes a good entry

- **Product ids from a real device.** A classic Bluetooth headset carries its Device ID profile's vendor and
  product ids in its profile nodes' instance ids (`..._VID&0001004C_PID&2014\...`); the first four digits say
  who assigned the vendor id (`0001` Bluetooth SIG, `0002` USB-IF). A name rule is the fallback, for devices
  without ids: the model's part of the name, never the user's ("AirPods Pro", not "Chris's AirPods Pro").
- **One entry per model.** Variants that look the same (a Lightning and a USB-C case) share an entry with both
  ids.
- **The shape the user sees.** `stem-buds`, `buds`, `over-ear` or `speaker`, checked with someone who owns it.

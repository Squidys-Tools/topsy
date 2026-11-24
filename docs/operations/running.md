# Running with test switches

Every `TOPSY_*` switch is a field in `src/config.rs`, read once at startup. A new switch is a field there and a
row here.

| Switch | What it does |
|---|---|
| `TOPSY_DEMO=1` | Expand and collapse the island by itself. |
| `TOPSY_LOG=1` | Event trace on stderr. |
| `TOPSY_EXIT_AFTER_MS=<n>` | Quit after `n` ms. |
| `TOPSY_NO_HOOK=1` | Don't install the mouse hook. The notch then rests peeking, as nothing could reveal it. |
| `TOPSY_NO_TRAY=1` | No tray icon and no menu, so right-clicking the notch quits. Start with Windows is left alone. |
| `TOPSY_NO_MEDIA=1` | Don't watch the system's media session: no Now Playing and no floating player. |
| `TOPSY_PLAYER=mini\|player\|expanded` | Open the floating player at launch at this size, in the middle of the screen. Its saved place is neither read nor written. |
| `TOPSY_NO_WIDGETS=1` | No shelf widgets in the tray, whatever the settings say (the live checks click the shelf). |
| `TOPSY_NO_KEYS=1` | Leave the volume keys to Windows, whatever the *Volume HUD* setting says. |
| `TOPSY_IDLE=peek\|tray` | What the notch rests as, instead of hidden. The live checks use it. |
| `TOPSY_SEED=<path;path>` | Start with these files on the shelf. |
| `TOPSY_TRANSFORM=<path;path>` | Open the Transform panel on these files at launch. |
| `TOPSY_STATE=<path>` | The shelf's save file, `%LOCALAPPDATA%\topsy\shelf.txt` by default (the basket's sits beside it). Set empty to not save. A run with it set also leaves the floating player's saved place alone. |
| `TOPSY_CLIPBOARD=<dir>` | Where clipboard history is kept, `%LOCALAPPDATA%\topsy\clipboard` by default. Set empty for none. A run with `TOPSY_STATE` set keeps none unless this says where, so a live check never touches the real history. |
| `TOPSY_LENS=off\|snapshot\|live` | The open tray's lens: none, the screen read once as it opens, or following the screen, whatever the *Live glass* setting says. See [Glass](../internals/glass.md). |
| `TOPSY_FAKE_DEVICE=<slug>[:<power>]` | Show a headset connecting at startup, for screenshots without the device. The slug is a catalog entry's (`airpods-pro-2`) or a shape's (`stem-buds`, `buds`, `over-ear`, `speaker`); the power is `80`, or `L80,R75,C40` for each bud and the case. See [Devices](devices.md). |
| `TOPSY_REPLACES=<pid>` | Wait for that process to exit before starting. An update's restart sets it on the new topsy; nothing else needs it. |
| `TOPSY_GLASS=host\|acrylic\|tint\|solid\|clear` | Force a fill and ignore the *Black notch* setting; the default is the notch-shaped live blur, and `host` holds it to the system's host backdrop. See [Glass](../internals/glass.md). |

The developer's own topsy may be running, and other worktrees run theirs. Stop only a PID you started; never
kill topsy by name (AGENTS.md, The ways to hurt yourself).

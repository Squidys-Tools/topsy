# topsy

A free, open-source take on [Droppy](https://getdroppy.app) for Windows: a glass notch at the top of the screen
that holds your files, shows what is playing and keeps your clipboard. Native Rust on
[GPUI](https://crates.io/crates/gpui), with real acrylic glass, no webview and no background polling.

## What it does

- **[A notch that stays out of the way.](./docs/user/notch.md)** Hidden until you rest the cursor on the top
  edge of the screen. It never takes the focus from the app you are in.
- **[A shelf for files.](./docs/user/shelf.md)** Drop files on the notch, then drag them out to any window
  later, as you would from Explorer.
- **[A basket that comes to you.](./docs/user/basket.md)** Jiggle a drag anywhere on screen and a floating file
  tray flies in under the pointer to catch it. Rename, share or hand over everything at once.
- **[Now Playing.](./docs/user/now-playing.md)** The art, title, progress and controls of whatever Windows is
  playing, with level bars that move with the music.
- **[Clipboard history.](./docs/user/clipboard-history.md)** Ctrl+Alt+V opens everything you copied: text,
  links, colours, code, images and files. Search, pin, paste.
- **[HUDs.](./docs/user/huds.md)** Volume, brightness and battery changes drop the notch down instead of
  Windows' flyouts.
- **[Live Activities.](./docs/user/live-activities.md)** A running download or Bluetooth headphones coming and
  going hold the notch down while they last.
- **[Transform.](./docs/user/transform.md)** Compress, convert, resize, trim, turn into a GIF or a PDF, strip
  metadata or cut out the background, all on your PC.
- **[Tray icon and settings.](./docs/user/settings.md)** Start with Windows, a *Send to* entry in Explorer, and
  a black notch if you prefer it to glass.

## Install

topsy is not released yet. To try it, download the latest build from CI:

1. Open the repository's **Actions** tab, pick the latest successful **build** run on `main`.
2. Download the `topsy-windows-gnu` artifact and unzip it.
3. Run `topsy.exe`. No installer and no admin rights are needed.

Turn on *Start with Windows* from the tray icon's menu to keep it running. [CI and the
artifact](./docs/operations/ci.md) explains how to build one for any branch.

## Build from source

topsy builds with the GNU Rust toolchain (pinned in `rust-toolchain.toml`), a MinGW-w64 `gcc` on `PATH`, and
`fxc.exe` from the Windows SDK (or `scripts/get-fxc.ps1`, no admin needed). No MSVC.

```powershell
cargo run --release
```

[Building](./docs/operations/building.md) has the details.

## Documentation

- [Using topsy](./docs/README.md#using-topsy): a page per feature.
- [Working on topsy](./docs/README.md#working-on-topsy): building, test switches, live checks, and the
  decisions and traps behind the design. Start with [AGENTS.md](./AGENTS.md).

## License

[MIT](LICENSE). The crates this builds on (GPUI and its dependency tree, 485 packages on Windows) are all
permissively licensed (MIT, Apache-2.0, BSD, Zlib, ISC and similar); a distributed binary should ship their
notices. The tools Transform downloads on first use (FFmpeg under the GPL, ImageMagick, oxipng, pdfium) are
separate programs fetched from their own releases, never shipped with topsy. The icons are
[Hugeicons](https://hugeicons.com)' free set (`@hugeicons/core-free-icons` 4.4.0, MIT, `assets/icons/LICENSE.md`),
their path data written out as SVG; play, pause, skip and the pin star are filled. A distributed binary should
ship that licence too.

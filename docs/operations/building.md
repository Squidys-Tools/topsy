# Building

topsy builds with the **GNU toolchain** (`x86_64-pc-windows-gnu`), pinned in `rust-toolchain.toml`. No MSVC.
Why `build.rs` does what it does is in [GNU toolchain and build.rs](../internals/toolchain.md).

```powershell
cargo run                  # then hold the left mouse button on a file and jiggle it side to side
cargo test <name>          # the tests for what you changed
cargo test -p topsy-model  # just the rules, in seconds: no GPUI to build
scripts/check.ps1          # fmt, clippy, the full test suite and structure: the gates CI runs
```

A cold build of GPUI takes about 10 minutes in a fresh worktree.

## What you need

- **rustup's `cargo` first on `PATH`.** A standalone Rust install (such as an MSVC one) ignores
  `rust-toolchain.toml`, and `build.rs` stops it.
- **A MinGW-w64 `gcc` on `PATH`** (e.g. `scoop install gcc`, or MSYS2). `build.rs` copies one import library
  from it and uses its `windres` for the icon.
- **`fxc.exe`** from the Windows SDK, which GPUI uses to compile its shaders. Set `GPUI_FXC_PATH` if it isn't
  found.

### No SDK, or no admin rights?

GPUI only needs `fxc.exe` and the `d3dcompiler_47.dll` beside it. `scripts/get-fxc.ps1` takes both from
Microsoft's SDK package on NuGet (no admin), checks their signatures, and sets `GPUI_FXC_PATH` for your user;
open a new terminal afterwards.

Or skip building altogether and download the exe CI builds; see [CI](ci.md).

## Debug and release

Use a release build (`cargo build --release`) to judge anything about focus or footprint. A debug build is a
console app, and the console window it opens takes foreground focus at launch. Release builds use the GUI
subsystem.

# GNU toolchain and `build.rs`

topsy builds with the GNU toolchain (`x86_64-pc-windows-gnu`), pinned in `rust-toolchain.toml`, never MSVC.
The binary imports `msvcrt.dll`, with no VCRUNTIME or MSVCP to install. How to set it up is in
[Building](../operations/building.md); this page is why `build.rs` looks the way it does.

- **One import library is copied, nothing else.** rustc's bundled MinGW ships only some of the Windows import
  libraries, and a GPUI dependency links `ktmw32`. `build.rs` asks the system MinGW-w64 `gcc` where `libktmw32.a`
  lives and copies that one file, so the build doesn't mix two MinGW runtimes.
- **Pointing rustc's linker at the system gcc does not work.** scoop's gcc has no `libgcc_eh.a`.
- **The icon is drawn in code and linked as a `.res`.** `build.rs` draws the icon (`src/model/app_icon.rs`),
  writes it as a binary `.res`, and the same MinGW's `windres` turns it into an object linked into the exe, so no
  `.rc` script and no preprocessor are involved. GPUI's windows and the tray icon load it as resource 1. It is
  linked into every link, tests included, so a test can check the icon made it in.
- **`build.rs` stops an MSVC build.** A standalone Rust install (such as an MSVC one) ignores
  `rust-toolchain.toml`; `build.rs` fails it rather than let it through without the icon.
- **GPUI needs `fxc.exe`** from the Windows SDK to compile its HLSL shaders. `GPUI_FXC_PATH` points it there.

# Measurements

An always-on app should cost close to 0% CPU at idle and render nothing when nothing changes.
`scripts/measure-idle.ps1` measures the footprint and `scripts/count-renders.ps1` counts the island's re-renders.
Measure a release build.

Last measured on a release build, GNU toolchain, Windows 11, one 3440x1440 display, idle:

| | |
|---|---|
| Working set | ~70 MB (private ~56 MB) |
| Threads | 16 (15 without the mouse hook), Now Playing's and WinRT's included |
| Binary | 37.1 MB (unstripped; the earlier MSVC build was 10.6 MB) |
| Idle CPU | ~1–2% of one core, with or without the hook or Now Playing (a busy machine; see below) |
| Island re-renders at idle | 0 over 10 s |

Idle CPU swings with machine load: the GNU build once measured ~0.5–0.7%, an earlier MSVC build ~1.5–2%, and
the build before Now Playing, the floating tray, chip actions and the settings window measured the same 0.7–2%
as the build after, back to back on a busy machine. The cause was never found (not our render loop, not the
hook, not the media thread). It is worth a profile.

The open tray's lens keeps a Direct3D device from startup: about 24 MB more private memory and two more threads,
no idle CPU (`TOPSY_LENS=off` leaves them out). A snapshot (the default) costs nothing while the tray is open.
With *Live glass* on and the tray open, topsy takes about 0.6% of a core and 2.3% of an integrated GPU's 3D
engine, and DWM 3.4% of it over a still backdrop or 5.9% over one animating every frame.

While music plays and its level bars are on screen, the meter reads add about 2.5–3% of a core; see
[Now Playing](../internals/now-playing.md).

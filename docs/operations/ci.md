# CI, the artifact and nightlies

CI (`.github/workflows/build.yml`) runs on a GitHub Windows runner, which has the Windows SDK and a MinGW gcc. It
runs the same gates as `scripts/check.ps1` (`scripts/check-structure.ps1`, `cargo fmt --check`, clippy with
`-D warnings` on all targets, the full `cargo test`, each across the workspace), in release, then builds the exe.
When it fails, read its log (`gh run view <id> --log-failed`) and fix the cause. `main` is kept green, so a
failure on a branch is that branch's.

## The `topsy-windows-gnu` artifact

CI builds the release exe and uploads it as the `topsy-windows-gnu` artifact for every push to `main` and for a
run started by hand, not for pull requests:

```powershell
gh workflow run build.yml --ref <branch>
```

or **Run workflow** on the repository's **Actions** tab. Download the artifact from that run: no Rust, MinGW or
SDK is needed to run it. A cold build takes about 20 minutes.

The developer often tests from a machine without admin rights using this artifact, so CI is part of the product:
keep it green and keep the artifact working.

## Nightlies

`.github/workflows/nightly.yml` publishes `main` as a GitHub prerelease every 8 hours, from midnight US Eastern
standard time (05:08, 13:08 and 21:08 UTC; cron has no time zone, so it is an hour later in local time during
daylight saving). A scheduled run publishes only when `main` has moved since the last nightly; a run by hand
(`gh workflow run nightly.yml`) always publishes. Setting the repository variable `PAUSE_NIGHTLY` to `true`
stops the scheduled runs.

It calls `build.yml`, so a nightly passes the same gates as every PR. The tag is
`v<next patch>-nightly.<UTC date>.<run number>`, as T3 Code's nightlies are, and the release carries `topsy.exe`
under that same name (so replacing it keeps *Start with Windows* pointing at it) and its `SHA256SUMS`. A
nightly is never marked latest, so a stable release later takes `releases/latest` without a change here.

The build stamps `TOPSY_RELEASE_VERSION` into the exe, which the settings page's About shows in place of the crate
version. It is a build-time variable, not a `TOPSY_*` run switch.

The exe is unsigned, so SmartScreen warns on first run. Signing needs a paid certificate; until then the release
notes tell people how to get past the warning.

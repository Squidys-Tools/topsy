# CI and the artifact

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

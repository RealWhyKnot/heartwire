# Contributing

You need a stable Rust toolchain. On Linux, install
`pkg-config libudev-dev libdbus-1-dev libssl-dev libfontconfig1-dev libxkbcommon-dev` first.

Turn on the repository's hooks after cloning:

```bash
git config --local core.hooksPath .githooks
```

They stamp the current build version onto the commit subject and reject a
subject that isn't a conventional commit.

## Where things live

| Path | What it does |
|---|---|
| `src/app/` | Start-up, single instance, and the glue between the engine and the window. `state.rs` holds what the UI thread owns |
| `src/engine/` | Turns readings into OSC and decides when the connection has dropped |
| `src/sources/` | One module per heart rate source: `auto`, `ble`, `http`, `pico`, `pulsoid`. `context.rs` is how a source reports and how it's stopped |
| `src/steamvr/` | SteamVR start-up registration and the dashboard panel. Every unsafe OpenVR call is in `openvr.rs` |
| `src/perf/` | The performance suite behind `--perf` |
| `src/update/` | Finding a newer release, the verified download, the install helper |
| `installer/` | The Windows setup (NSIS), the script that builds it and the script that tests it |
| `src/config/` | Settings file, defaults, and the one-time import from hr-osc |
| `src/platform/` | Start at login and opening links, per operating system |
| `ui/app.slint` | The window, composed from `ui/pages` and `ui/widgets` |
| `ui/state.slint` | Globals the Rust side reads and writes: `HeartRate`, `Settings`, `Updates`, `Navigation` |
| `src/ui/tests/` | Renders every screen offscreen and checks the layout against hr-osc |

Unit tests sit at the bottom of the file they cover. Set `HEARTWIRE_UI_DUMP` to a
folder when running `cargo test` and every screen the UI tests render is saved
there as a PNG.

## Checks

CI runs these on Windows, macOS and Linux. Run them before opening a pull request:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The tests don't need a strap, a Pico or Bluetooth hardware.

A few tests are ignored by default because they need something this list doesn't
cover: SteamVR installed and closed, a Pico plugged in, the network, or a desktop
with a visible taskbar. `tests/taskbar_beat.rs` is the last kind. It runs the app
minimized with a fake 120 bpm feed and a Start menu shortcut pointing at it, then
watches the taskbar pixels for the beat:

```bash
cargo test --release --test taskbar_beat -- --ignored --nocapture
```

`installer/build.ps1` packs a folder with the zip's files into the Windows setup.
It needs [NSIS](https://nsis.sourceforge.io) 3. CI builds one from a debug build,
then `installer/test.ps1` installs, moves, reinstalls and uninstalls it under the
current account. The test won't run where Heartwire is already installed or
starts with Windows.

```powershell
./installer/build.ps1 -Payload dist/payload -Version 0.0.0.0-dev -OutFile dist/heartwire-setup.exe
./installer/test.ps1 -Setup dist/heartwire-setup.exe -Payload dist/payload -Version 0.0.0.0-dev
```

## Performance

`heartwire --perf report.txt` times every part of the app: parsing, OSC, the
engine, each source, settings, update checks, platform probes and the window
and dashboard rendering. Each case has a budget and the report marks any that go
over. Run it on a release build. Debug numbers are several times slower.

## Commits

Conventional subjects: `type(scope): description`. Keep them to one line unless
the change needs context the subject can't hold.

## Releases

Tag `vYYYY.M.D.N` and push the tag. The release workflow checks the tag is the
next one for its day, runs the tests, builds every platform and publishes the
archives and the Windows setup with notes built from the commit subjects since
the last release.

```bash
git tag v2026.10.9.0
git push origin v2026.10.9.0
```

A `-beta` suffix publishes a prerelease. The nightly job does that by itself
when main has a feature or fix since the last tag.

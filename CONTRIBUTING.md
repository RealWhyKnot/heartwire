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
| `src/app/` | Start-up, single instance, and the glue between the engine and the window |
| `src/engine.rs` | Turns readings into OSC and decides when the connection has dropped |
| `src/sources/` | One module per heart rate source: `auto`, `ble`, `http`, `pico`, `pulsoid` |
| `src/steamvr/` | Loading SteamVR's OpenVR library, registration, the dashboard panel |
| `src/update/` | Finding a newer release, the verified download, the install helper |
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

## Commits

Conventional subjects: `type(scope): description`. Keep them to one line unless
the change needs context the subject can't hold.

## Releases

Tag `vYYYY.M.D.N` and push the tag. The release workflow checks the tag is the
next one for its day, runs the tests, builds every platform and publishes the
archives with notes built from the commit subjects since the last release.

```bash
git tag v2026.10.9.0
git push origin v2026.10.9.0
```

A `-beta` suffix publishes a prerelease. The nightly job does that by itself
when main has a feature or fix since the last tag.

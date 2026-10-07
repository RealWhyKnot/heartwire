# Contributing

You need a stable Rust toolchain. On Linux, install
`pkg-config libudev-dev libdbus-1-dev libssl-dev libfontconfig1-dev libxkbcommon-dev` first.

Turn on the repository's hooks after cloning:

```bash
git config --local core.hooksPath .githooks
```

They stamp the current build version onto the commit subject and reject a
subject that isn't a conventional commit.

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

# Heartwire

Heavily inspired by [hr-osc](https://github.com/kamyu1537/hr-osc) by kamyu.

Sends your heart rate to VRChat over OSC. It reads a Bluetooth chest strap or
watch directly, a Raspberry Pi Pico W plugged in over USB, HTTP posts from
another app, or a Pulsoid widget.

The window and settings look like hr-osc's, and the OSC parameters have the
same names. Avatars built for hr-osc work unchanged. Heartwire is a separate
program written from scratch. It replaces my two bridges, hr-bridge-ble and
hr-bridge-pico: both jobs now happen inside the app.

## Install

On Windows, download `heartwire-<version>-win-x64-setup.exe` from
[Releases](https://github.com/RealWhyKnot/heartwire/releases) and run it. It
installs for your account only and doesn't need admin rights. Uninstall it from
Settings > Apps > Installed apps, which also takes it off SteamVR's app list.
The uninstaller asks before deleting your settings.

For a portable copy, or on Linux and macOS, download the archive for your system,
unpack it anywhere and run `heartwire.exe` (or `heartwire`).

The app checks GitHub for a newer release when it starts and offers to update
itself. Untick "Check for updates" under Settings > General to turn that off.

If hr-osc has been used on this computer, Heartwire offers to bring its settings
over the first time it starts. Close hr-osc first: both want port 8080 in HTTP mode.

## Heart rate sources

Pick one under Settings > General > Service Type.

| Service | Reads from |
|---|---|
| Auto | The Pico if one is plugged in, otherwise the computer's Bluetooth. This is the default. |
| Bluetooth | Any strap or watch broadcasting the standard heart rate service (`0x180D`). |
| Pico (USB) | A Pico W or Pico 2 W running the firmware in `firmware/`. |
| HTTP | A POST holding a bare number, sent to port 8080. |
| Pulsoid / Stromno | A Pulsoid widget ID. |

The Home tab shows the reading and, under the connection status, which device it
came from.

### Bluetooth

Settings > Bluetooth lists every heart rate device in range. Leave it on "Any
heart rate device" to take the strongest one, or click a device to stick to it.
Watches only show up when they broadcast heart rate over Bluetooth. Most chest
straps do it whenever they touch skin.

A strap takes one connection at a time. If it's already connected to a phone or
a Pico, the computer won't see it.

### Pico bridge

For computers without Bluetooth, a Raspberry Pi Pico W does the Bluetooth work
and sends readings over its USB cable.

1. Hold BOOTSEL while you plug the board in. It shows up as a drive called
   `RPI-RP2`. Drop the MicroPython `.uf2` for your board on it:
   [Pico 2 W](https://micropython.org/download/RPI_PICO2_W/) or
   [Pico W](https://micropython.org/download/RPI_PICO_W/).
2. Open Settings > Pico in Heartwire and click Install Firmware. Put part of
   the strap's name in Strap Name first if there's more than one strap nearby.
3. Put the strap on. The board's LED lights up once it's connected.

You can also copy the firmware by hand with
`mpremote connect auto fs cp firmware/main.py :main.py`.

On Linux your account needs access to the serial port. On most distributions
that's the `dialout` group: `sudo usermod -a -G dialout $USER`, then log out and
back in.

### HTTP

```bash
curl -X POST -d '60' http://localhost:8080
```

The server listens on every network interface. A phone app on the same network
can post to your computer's address.

## VRChat parameters

| Parameter | Type | Value |
|---|---|---|
| `hr_connected` | Bool | True while readings arrive. False after Connected Timeout seconds without one. |
| `hr_percent` | Float | Heart rate divided by Max Heart Rate, from 0 to 1. |

Both paths can be changed under Settings > Parameters. The OSC target is
`127.0.0.1:9000` by default.

## SteamVR

On Windows, tick "Start with SteamVR" under Settings > General. SteamVR then
starts Heartwire along with itself, and a heart icon shows up in the SteamVR
dashboard. The panel behind it shows your heart rate and where it's coming
from. If SteamVR is closed when you tick the box, the app registers itself the
next time SteamVR runs. Untick it to take Heartwire off SteamVR's start-up
list again.

When SteamVR started the app, closing SteamVR closes it too. A copy you opened
yourself keeps running.

## Taskbar

While readings arrive, the taskbar icon beats along with your heart rate.

## Start at login

Tick "Start at login" under Settings > General. The app then starts minimized
when you sign in. On Windows that's an entry under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. If you move the program
folder, open it once from the new place and the entry follows.

## Files

Settings and the log live in `%APPDATA%\heartwire` on Windows,
`~/.config/heartwire` on Linux and `~/Library/Application Support/heartwire`
on macOS.

## Building

```bash
cargo build --release
```

Linux needs `pkg-config libudev-dev libdbus-1-dev libssl-dev libfontconfig1-dev libxkbcommon-dev`.

Checks run on every push:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Releases are tagged `vYYYY.M.D.N`. Pushing a tag builds Windows, Linux and macOS
archives and publishes them. A nightly job tags a `-beta` when main has a new
feature or fix since the last tag, and beta builds update to newer betas.

## License

GPL-3.0. The font and icon licences are in [NOTICE.md](NOTICE.md).

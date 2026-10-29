<p align="center"><img src="assets/nubmiddle.png" width="128" alt="nubmiddle icon: a red TrackPoint nub between four keycaps"></p>

# nubmiddle

[日本語](README.ja.md)

A tiny Windows tray tool that gives the middle button of a TrackPoint-style pointing stick its
middle click back, without giving up scrolling:

- **Click** the middle button → a normal middle click (close tab, open link in new tab, ...)
- **Hold** the middle button and **push the stick** → scroll vertically / horizontally

Lenovo's official driver forces you to choose one or the other, and recent ThinkPads default to scrolling. Linux (Xorg / libinput) has done both
out of the box for years; nubmiddle brings the same behavior to Windows, with nothing else bundled.

> **Status: early release.** See [Install](#install). The exe is not code-signed yet, so
> Windows SmartScreen may warn on first start ("More info" → "Run anyway").

### Supported devices

nubmiddle works on the OS mouse-event level, so it should work with any pointing stick whose middle
button reaches Windows as a real middle button. Tested with ThinkPad TrackPoint keyboards
(SK-8855 and TrackPoint Keyboard II). Other sticks such as the HHKB Studio are expected to work but
are untested — reports are welcome.

## Why another tool?

Existing options each fall short in some way:

| Tool | Problem |
|---|---|
| TPMiddle | Depends on the Synaptics API; does not work with ELAN TrackPoints (X390 and later). Unmaintained. |
| W10Wheel.NET | Works, but is a general wheel simulator with many unrelated features. Autostart with admin rights must be set up by hand. |
| X-Mouse Button Control / AutoHotkey | Possible, but general-purpose and need manual configuration. |

nubmiddle aims to be single-purpose, vendor-independent (it works on the OS mouse event level, not a
vendor API), and to manage its own autostart — including fixing it up automatically when the exe is moved.

## Install

Download from [Releases](https://github.com/akivajp/nubmiddle/releases) — `x64` for most PCs,
`arm64` for Windows on ARM:

- **Installer** (`nubmiddle-vX.Y.Z-setup-x64.exe`): installs for the current user without administrator
  rights (or for all users if you choose so), adds a Start menu entry and can set up starting with
  Windows. Running a newer installer updates in place. Uninstall from Windows Settings → Apps; your
  settings are kept unless you choose to delete them.
- **Portable** (`nubmiddle-vX.Y.Z-x64.zip`): extract anywhere and run `nubmiddle.exe`. Put an empty
  `nubmiddle.ini` next to it to keep the settings there too.

## Usage

```
nubmiddle [OPTIONS]
```

nubmiddle stays in the notification area (system tray) without a console window. Its menu has:

- **Middle button + move**: *Scroll* (default) or *Middle-button drag* (no scrolling; for panning in
  CAD, Blender, ...). A middle click works the same in both.
- **Vertical / Horizontal scroll speed**: five levels each, from x0.5 to x2 of the default.
- **Devices that scroll**: all mice and pointing devices (default), or only the devices checked in the
  list of connected devices. Other devices keep a plain middle button, so e.g. a mouse can still
  middle-drag in Blender while the TrackPoint scrolls. Devices are told apart with Raw Input and saved
  by their vendor and product id (e.g. `17EF:6009`).
- **Reset to defaults**, **Open settings file**, **Reload settings file**
- **Language**: English or Japanese; by default it follows the Windows display language

- **Start with Windows (as administrator)**: registers a Task Scheduler task that starts nubmiddle at
  logon with administrator rights (one UAC prompt when toggling, none at logon). The task reuses the
  options of the running instance. If the exe has been moved, the item turns into
  "update to this exe"; an instance running as administrator fixes the task by itself.
- **Pause**: lets every mouse event through until unchecked.
- **Quit**

Only one instance runs at a time. When started from a console, `--help` and the log are printed there.

### Settings file

Changes made in the menu take effect at once and are saved to `nubmiddle.ini`:

- `nubmiddle.ini` next to the exe, if that file exists (portable use, e.g. an exe kept in a synced folder);
- otherwise `%APPDATA%\nubmiddle\nubmiddle.ini`;
- or the file given with `--config <FILE>`.

Each line is `key = value` with the keys `mode`, `threshold`, `sensitivity_v`, `sensitivity_h`,
`horizontal`, `axis_lock`, `invert_v`, `invert_h`, `quantize`, `cursor` and `hold_delay_ms`
(see the options below for their meaning), `language` (`auto`, `en` or `ja`) and `devices`
(`all`, or ids separated by commas such as `17EF:6009, 04FE:0021`). Command-line options take precedence over the file, until the
same setting is changed in the menu. Note that the autostart task starts nubmiddle with the options of
the instance that registered it.

| Option | Description |
|---|---|
| `--mode <scroll\|drag>` | What middle button + move does (default: `scroll`). `drag` passes it through as a normal middle-button drag. |
| `--threshold <PX>` | Movement distance beyond which it is no longer a middle click (default: 4) |
| `--sensitivity <N>` | Wheel amount per pixel for both axes; 120 = one notch (default: 10) |
| `--sensitivity-v <N>` / `--sensitivity-h <N>` | Same, for the vertical / horizontal axis only |
| `--no-hwheel` | Disable horizontal scrolling |
| `--no-axis-lock` | Scroll both axes at once. By default each scroll is locked to the axis it started along, because sideways drift mixed into vertical scrolling makes some apps (e.g. Windows Settings) shake |
| `--invert-v` / `--invert-h` | Invert vertical / horizontal scroll direction |
| `--quantize` | Send wheel events only in whole notches (for apps that mishandle high-resolution wheel input) |
| `--no-cursor` | Do not change the mouse cursor while scrolling. By default it becomes a vertical / horizontal arrow showing the scroll axis |
| `--hold-delay <MS>` | How long the middle button must be held before the cursor shows the four-way scroll arrow, so a quick middle click does not flash it (default: 200) |
| `--config <FILE>` | Settings file to use (see [Settings file](#settings-file)) |
| `--log <FILE>` | Also append the log to a file |
| `--quit` | Quit the running nubmiddle and wait until it has exited (used by the installer) |
| `--trace` | Log middle button / wheel events in detail, including events injected by drivers (for troubleshooting) |
| `-h`, `--help` | Show help |

### Driver setting

Set the middle button to **"middle click"** (not "scroll") in the Lenovo / Synaptics / ELAN driver settings.
Otherwise the driver may convert the middle button into wheel events itself, and nubmiddle never sees it.
`--trace` shows what the OS actually receives: `source=device` is a physical event, `source=injected`
is an event synthesized by other software or a driver.

### Elevated windows

Because of Windows UIPI, a normal process cannot scroll windows running as administrator
(e.g. an elevated terminal). Run nubmiddle as administrator to make it work there too;
"Start with Windows" in the tray menu does this for you.

## How it works

1. A low-level mouse hook (`WH_MOUSE_LL`) swallows the middle-button down event and holds it.
2. While held, mouse moves are swallowed (the cursor stays put) and accumulated.
   Once they exceed the threshold, the movement is converted into wheel events with `SendInput`.
3. If the button is released before that, the held down/up pair is replayed as a middle click.

Events sent by nubmiddle itself are tagged via `dwExtraInfo` so the hook does not process them again.

## Building

nubmiddle is written in Rust. It can be cross-compiled from Linux / WSL:

```sh
sudo apt install mingw-w64
rustup target add x86_64-pc-windows-gnu
cargo win --release          # alias for: cargo build --target x86_64-pc-windows-gnu
# → target/x86_64-pc-windows-gnu/release/nubmiddle.exe
```

The decision logic is OS-independent, so the tests run on any platform:

```sh
cargo test
```

The icons (`assets/nubmiddle.ico`, `assets/nubmiddle-paused.ico`) are generated by a script and
embedded into the exe at build time. To change them, edit and rerun the script:

```sh
uv run assets/make_icons.py --preview preview.png
```

## Roadmap

- [x] Core: middle click / scroll / drag decision, wheel output (console prototype)
- [x] Scroll cursor: show a vertical / horizontal arrow cursor while scrolling, so the locked axis is visible
- [x] Tray icon with menu: autostart (checkbox), pause (checkbox), settings, quit
- [x] Autostart via Task Scheduler with highest privileges, toggled from the tray menu
- [x] Self-repair: re-register the task when the exe has been moved or replaced
- [x] Settings file (ini next to the exe, for portable use)
- [ ] Per-process exclusion list (Blender, CAD, ...)
- [x] Apply only to chosen devices (e.g. the TrackPoint, not external mice), using Raw Input
- [x] Release builds for x64 / ARM64 via GitHub Actions
- [x] Installer (in addition to the single portable exe)
- [ ] Code-signed releases (e.g. through SignPath Foundation, once the project qualifies)

## License

[MIT](LICENSE) © 2026 Akiva Miura

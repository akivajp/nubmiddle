# nubscroll

[日本語](README.ja.md)

A tiny Windows tray tool that makes the ThinkPad TrackPoint middle button do **both**:

- **Click** the middle button → a normal middle click (close tab, open link in new tab, ...)
- **Hold** the middle button and **push the TrackPoint** → scroll vertically / horizontally

Lenovo's official driver forces you to choose one or the other. Linux (Xorg / libinput) has done both
out of the box for years; nubscroll brings the same behavior to Windows, with nothing else bundled.

> **Status: early prototype.** It currently runs as a console program. The tray icon, the built-in
> autostart toggle (with administrator rights) and the settings file are not implemented yet —
> see [Roadmap](#roadmap).

## Why another tool?

Existing options each fall short in some way:

| Tool | Problem |
|---|---|
| TPMiddle | Depends on the Synaptics API; does not work with ELAN TrackPoints (X390 and later). Unmaintained. |
| W10Wheel.NET | Works, but is a general wheel simulator with many unrelated features. Autostart with admin rights must be set up by hand. |
| X-Mouse Button Control / AutoHotkey | Possible, but general-purpose and need manual configuration. |

nubscroll aims to be single-purpose, vendor-independent (it works on the OS mouse event level, not a
vendor API), and to manage its own autostart — including fixing it up automatically when the exe is moved.

## Usage

```
nubscroll [OPTIONS]
```

| Option | Description |
|---|---|
| `--mode <scroll\|drag>` | What middle button + move does (default: `scroll`). `drag` passes it through as a normal middle-button drag. |
| `--threshold <PX>` | Movement distance beyond which it is no longer a middle click (default: 4) |
| `--sensitivity <N>` | Wheel amount per pixel; 120 = one notch (default: 10) |
| `--no-hwheel` | Disable horizontal scrolling |
| `--no-axis-lock` | Scroll both axes at once. By default each scroll is locked to the axis it started along, because sideways drift mixed into vertical scrolling makes some apps (e.g. Windows Settings) shake |
| `--invert-v` / `--invert-h` | Invert vertical / horizontal scroll direction |
| `--quantize` | Send wheel events only in whole notches (for apps that mishandle high-resolution wheel input) |
| `--no-cursor` | Do not change the mouse cursor while scrolling. By default it becomes a vertical / horizontal arrow showing the scroll axis |
| `--log <FILE>` | Also append the log to a file |
| `--trace` | Log middle button / wheel events in detail, including events injected by drivers (for troubleshooting) |
| `-h`, `--help` | Show help |

### Driver setting

Set the middle button to **"middle click"** (not "scroll") in the Lenovo / Synaptics / ELAN driver settings.
Otherwise the driver may convert the middle button into wheel events itself, and nubscroll never sees it.
`--trace` shows what the OS actually receives: `source=device` is a physical event, `source=injected`
is an event synthesized by other software or a driver.

### Elevated windows

Because of Windows UIPI, a normal process cannot scroll windows running as administrator
(e.g. an elevated terminal). Run nubscroll as administrator to make it work there too.
The planned autostart feature will do this for you.

## How it works

1. A low-level mouse hook (`WH_MOUSE_LL`) swallows the middle-button down event and holds it.
2. While held, mouse moves are swallowed (the cursor stays put) and accumulated.
   Once they exceed the threshold, the movement is converted into wheel events with `SendInput`.
3. If the button is released before that, the held down/up pair is replayed as a middle click.

Events sent by nubscroll itself are tagged via `dwExtraInfo` so the hook does not process them again.

## Building

nubscroll is written in Rust. It can be cross-compiled from Linux / WSL:

```sh
sudo apt install mingw-w64
rustup target add x86_64-pc-windows-gnu
cargo win --release          # alias for: cargo build --target x86_64-pc-windows-gnu
# → target/x86_64-pc-windows-gnu/release/nubscroll.exe
```

The decision logic is OS-independent, so the tests run on any platform:

```sh
cargo test
```

## Roadmap

- [x] Core: middle click / scroll / drag decision, wheel output (console prototype)
- [x] Scroll cursor: show a vertical / horizontal arrow cursor while scrolling, so the locked axis is visible
- [ ] Tray icon with menu: autostart (checkbox), pause (checkbox), settings, quit
- [ ] Autostart via Task Scheduler with highest privileges, toggled from the tray menu
- [ ] Self-repair: re-register the task when the exe has been moved or replaced
- [ ] Settings file (ini next to the exe, for portable use)
- [ ] Per-process exclusion list (Blender, CAD, ...)
- [ ] Apply only to the TrackPoint, not to external mice (Raw Input)
- [ ] Release builds for x64 / ARM64 via GitHub Actions
- [ ] Installer (in addition to the single portable exe)

## License

[MIT](LICENSE) © 2026 Akiva Miura

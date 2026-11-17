# WallFX

Memory-safe live wallpaper engine for Windows (Rust).

**Repo:** https://github.com/berithjeshurun/wallfx  
**Windows release:** https://github.com/berithjeshurun/wallfx/releases/tag/v0.1.0

Download `WallFX-windows-x64-v0.1.0.zip`, unzip, run `wallfx.exe`.

## Why Rust

- Always-on desktop process → memory safety matters more than Python speed of writing
- Player runs in a **separate process** (mpv / ffmpeg) so decoder crashes do not take down Explorer
- Desktop attach uses the **WorkerW sibling host** (not the icon layer) and always detaches on exit

## Default wallpapers

Eleven stock clips live in `wallpapers\` and are also zipped for shipping:

- Folder: [`wallpapers\`](wallpapers/) (`2`, `20`, `25`, `50`, `64`, `69`, `81`, `91`, `106`, `109`, `111`)
- Zip: [`dist\wallpapers-default.zip`](dist/wallpapers-default.zip) (~368 MB)

Default config points at `wallpapers/2.mp4`.

### Parallax (depth) wallpapers

Supports mylivewallpapers-style packs: a color image + `*-depth.jpg` (or a `.zip` containing both).

Example included: `wallpapers/parallax/argent-tower.zip`

In Settings set **Mode** to `Parallax` or `Auto`, pick the zip/image, tune **Parallax → Strength**, then Enable. Move the mouse to see the depth shift.

### Per-monitor sources

In Settings → **Monitors (per-display video)** assign a different video or parallax pack to each display (or leave blank to use the default).

### Color grade + LUT

Settings → **Color grade + LUT**:

- Brightness / contrast / saturation / gamma (ffmpeg `eq`)
- Optional `.cube` / `.3dl` LUT (`lut3d`) — drop files in `luts\`
- **Bake** writes a graded MP4 under `cache\graded\` (first Apply is slower; playback is lighter)

Live grade applies as an ffmpeg/mpv filter without rewriting the file.

## Tray app (default)

```powershell
cargo run --release
# or
.\target\release\wallfx.exe
.\target\release\wallfx.exe tray
```

System tray menu:

- **Enable / Disable** — start or stop the wallpaper from `config.yaml`
- **Settings** — opens the configured UI toolkit
- **Exit**

Double-click the tray icon to open Settings.

### Settings UI toolkit

In `config.yaml`:

```yaml
ui:
  toolkit: egui   # or: win32
```

Both the egui window and the Native Win32 dialog expose the same fields, including a **Settings UI** dropdown so you can switch toolkits. Apply, then reopen Settings to use the other UI.

Settings include: video path, loop, mute, preview, start with Windows, backend (`default` / `auto` / `mpv` / `ffmpeg`), monitors + time sync, fit/resolution caps, unlock fade, and `self.expand`.

## Backends

1. **mpv** (recommended) — GPU decode; looked up under `%CD%\bin\`, then `tools\`
2. **ffmpeg** (fallback) — software GDI blit from the same paths

## Self-contained pack

```powershell
cargo build --release
cargo run --release --bin pack
```

Appends a zstd overlay (mpv + ffmpeg + thin PE) onto `target\release\wallfx.exe`.

**Ship folder** for another PC:

| Item | Notes |
|------|--------|
| `wallfx.exe` | packed release binary |
| `config.yaml` | `self.expand: true` on first run |
| `wallpapers\` | or unzip `dist\wallpapers-default.zip` |
| `luts\` | optional LUTs |

First run extracts `bin\mpv.exe` / `bin\ffmpeg.exe` and shrinks the exe.

## CLI (scripts)

```powershell
cargo run --release -- set .\media\10.mp4
cargo run --release -- set .\media\10.mp4 --audio
cargo run --release -- stop
cargo run --release -- status
```

## Layout

```
src/          Rust engine + tray + dual settings UIs
tools/        ffmpeg.exe (+ optional mpv.exe) — develop / pack
bin/          extracted player tools after self.expand
wallpapers/   default clips
dist/         wallpapers-default.zip for shipping
luts/         optional .cube / .3dl LUTs
cache/graded/ baked graded MP4s
config.yaml   ui, wallpaper, grade, startup, unlock, self.expand
media/        full sample library
```

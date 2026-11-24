# ReClip

A lightweight personal game-clipping tool for Linux (tested on CachyOS with KDE Plasma Wayland and an AMD RX 6600). It keeps a rolling replay buffer, saves clips with a global hotkey, and provides a small gallery for trimming and exporting.

ReClip uses [gpu-screen-recorder](https://github.com/dec05ea/gpu-screen-recorder) as its capture/encoding engine; it does not implement its own capture pipeline.

## Features

- Automatic game detection from Steam library manifests (`appmanifest_*.acf`) plus user-defined executable paths.
- Recording starts when a game launches and stops when it closes.
- Global hotkey **Alt+S** saves the replay buffer (via the XDG GlobalShortcuts portal).
- KDE notification and a short beep on save.
- System tray icon (StatusNotifierItem) with **Save clip** and **Quit ReClip**.
- GTK4 gallery with an embedded player, a single scrub/trim bar with draggable start/end brackets, frame preview popups, and an Export dialog:
  - **Full size** (stream copy, no re-encode)
  - **10 MB / 20 MB / 50 MB / custom** target size (two-pass libx264 at low CPU priority)
  - Warns when the requested size is too small for the clip duration.
- Configurable buffer length, quality preset, audio sources, and save folder.

## Requirements

- Rust (2021 edition).
- `gpu-screen-recorder`, `gsr-cli`, `ffmpeg`, `ffprobe`.
- GTK4 >= 4.12, GStreamer with `gst-plugins-good` (needed by `GtkVideo` to decode MP4).
- `xdg-desktop-portal` with the KDE backend (`plasma-xdg-desktop-portal-kde.service`) for the global hotkey and notifications.
- A session bus and PipeWire.

## Build

```sh
cargo build
# or
cargo build --release
```

## Configuration

Config file: `~/.config/reclip/config.json` (created manually on first use):

```json
{
  "buffer_seconds": 30,
  "quality": "medium",
  "audio_mode": "game_plus_mic",
  "save_folder": "~/Videos/ReClip",
  "manual_games": []
}
```

- `quality`: `high` (20000 kbps), `medium` (12000 kbps), `economy` (8000 kbps).
- `audio_mode`: `game_only` or `game_plus_mic`.
- `manual_games`: list of executable paths for games outside Steam.

## Usage

```sh
target/debug/reclip start          # daemon: detect games, hotkey, tray
target/debug/reclip stop           # stop recording
target/debug/reclip save           # save the current replay manually
target/debug/reclip detect         # print the currently detected game
target/debug/reclip gallery        # open the gallery/trim/export window
target/debug/reclip autostart      # install and enable a systemd user unit
```

Saved clips go to `~/Videos/ReClip/` as `ReClip_YYYY-MM-DD_HH-MM-SS.mp4`.

### Gallery

Click a clip to play it inside the window. On the control bar:

- Drag the **teal brackets** to set the trim start/end.
- Drag elsewhere on the bar to seek; hover to see a thumbnail preview.
- Click **Export** and choose full size or a target size in MB.

The clip list refreshes automatically every two seconds.

### Hotkey

Press **Alt+S** while a game is running. The KDE portal may ask for permission the first time the shortcut is bound.

## Known limitations

- On Wayland, `gpu-screen-recorder` cannot capture a single game window (X11 only). ReClip records the whole monitor, but starts/stops automatically based on the detected game.
- Game detection is process/path-based: it matches Steam install directories and `manual_games` paths. It cannot detect games launched outside those sources.
- Steam helper apps (Proton, Steam Linux Runtime, Redistributables) are explicitly excluded.
- The recording buffer is written to disk; rapid repeated saves are queued by retrying until the engine is free again.
- No settings window yet; edit the JSON config file directly.

## License

See repository files. This project is provided as-is, for personal use.

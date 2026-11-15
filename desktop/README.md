# Kanálik for GNOME

Native TV application for Linux, built with Rust, GTK4 and libadwaita.

## Watching TV

- **Live TV is the primary screen**, with a searchable channel list on the left
  and an embedded GTK/GStreamer video player on the right.
- Switch channels by clicking on the list or using Up/Down.
- Playback controls: pause/resume (Space), previous/next channel, fullscreen (F11 / Esc).
- Open the current channel in **mpv** as a fallback via the external-player button.
- Channels requiring custom HTTP headers (such as Referer) open in mpv automatically,
  since the GTK video widget does not expose those headers.
- Automatically resumes the last selected channel at startup.
- Uses the same live catalog as Android: https://plainraw.com/json/cbc833011422.
  Keeps an offline cache if loading fails.

## Diagnostics: Stream Checker

Use the **⋮ application menu → Check stream availability…** to open the checker.
It is a separate secondary window; it does not interrupt TV playback.

- Checks all direct channels in the JSON using three concurrent **mpv** probes.
- Attempts to render 12 video frames with no visible output or audio.
- Reports Online, Failed or Unknown (timeout) with the elapsed time and error details.
- Uses the same mpv decoder and HTTP headers as external playback.
- Timeout is *inconclusive*, not an automatic failure.

## Requirements and run

GTK4 4.12+, libadwaita 1.4+, Rust toolchain, **mpv** and GStreamer playback
plugins. MPV is also required for checks.

On Pop!_OS / Ubuntu:

    sudo apt install libgtk-4-dev libadwaita-1-dev pkg-config mpv \
      gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
      gstreamer1.0-plugins-bad gstreamer1.0-libav

    git fetch origin feature/gnome-stream-checker
    git switch feature/gnome-stream-checker
    cd desktop
    cargo run --release

The embedded GTK player depends on installed GStreamer codecs and HTTP/HLS
plugins. If it cannot open a channel, use the external mpv playback button.

## Quality checks

    cd desktop
    cargo fmt --all --check
    cargo test
    cargo clippy --all-targets -- -D warnings

This app is independent of the Android build. EPG, archive and Sweet.tv
providers are not yet ported. It currently uses direct channels in the
remote JSON catalog.

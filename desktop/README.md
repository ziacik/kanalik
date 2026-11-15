# Kanálik for GNOME (prototype)

Native GTK4/libadwaita desktop companion to the Android TV application. The first
milestone focuses on validating all direct stream URLs and launching playback.

## Features

- Reads the exact same live channel catalog as the Android app:
  https://plainraw.com/json/cbc833011422
- Keeps an offline cache if the catalog server is unavailable.
- Search channels, copy a URL or open any channel in **mpv**.
- **Check All** probes 6 channels concurrently using **ffmpeg**, with a 12-second
  per-stream process timeout.
- Checks **actual decoded media output**, not just an HTTP 200 response or
  a downloadable HLS manifest.
- Preserves per-channel HTTP headers such as Referer in ffmpeg and mpv.
- Shows online/failed/timeout status, elapsed time and detailed error on hover.

EPG, archive and Sweet.tv-specific resolvers are not yet implemented. The
check currently covers every direct channel in the remote JSON catalog.

## Run on Linux

Dependencies: Rust toolchain, GTK4 4.12+, libadwaita 1.4+, pkg-config,
**ffmpeg** and **mpv**.

Arch / Manjaro:

    sudo pacman -S gtk4 libadwaita pkgconf ffmpeg mpv
    cd desktop
    cargo run --release

Debian/Ubuntu (a modern Rust toolchain from rustup is recommended):

    sudo apt install libgtk-4-dev libadwaita-1-dev pkg-config ffmpeg mpv
    cd desktop
    cargo run --release

Both ffmpeg and mpv are runtime dependencies. If either is absent, the
corresponding action reports a clear error.

## Development checks

    cd desktop
    cargo test
    cargo clippy --all-targets -- -D warnings

The Android code is unaffected; the GNOME application is its own Rust binary.

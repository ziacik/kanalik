# Kanálik for GNOME

Native TV app for Linux: Rust + GTK4/libadwaita, with an **embedded
libmpv player** (GTK GLArea/OpenGL rendering) compatible with GNOME/Wayland.

## TV first

- Live TV screen: channel list on the left, **mpv video inside the app**
  on the right. No GStreamer/GTK Video player.
- Built-in playback supports HLS and HEVC and applies channel-specific HTTP
  headers (including Referer and User-Agent).
- Click a channel, Up/Down to switch, Space to pause, F11 for fullscreen.
- Remembers the last watched channel and starts it when the catalog loads.
- External mpv button remains available as a fallback.
- Uses the Android application's live catalog
  https://plainraw.com/json/cbc833011422, with offline cache.

## Diagnostic stream checker

Open the app menu and choose **Check stream availability…**.

- Checks three channels concurrently with headless **mpv**, using exactly
  the same stream URLs and HTTP headers.
- A successful sample = online. Timeout = **unknown**, not failed.
- Details and elapsed times are shown in the secondary window.
- Checking does not interrupt TV playback.

## Manjaro / Arch Linux

Install the GTK development libraries, libmpv, and Rust toolchain:

    sudo pacman -S --needed base-devel pkgconf rust gtk4 libadwaita mpv libglvnd

Run from the Git repository:

    git fetch origin feature/gnome-stream-checker
    git switch feature/gnome-stream-checker
    cd desktop
    cargo run --release

The embedded player uses libmpv via GTK4's GLArea/OpenGL and EGL, not X11's
window ID, so it is suitable for Wayland. On distributions where
libmpv development files are packaged separately, install them as well.

## CI

    cargo fmt --all --check
    cargo test
    cargo clippy --all-targets -- -D warnings

EPG, archive and Sweet.tv resolvers are not yet ported to the desktop app.

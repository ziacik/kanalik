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
- By default, uses the Android application's live catalog
  https://plainraw.com/json/cbc833011422, with offline cache.
- **Load channels JSON…** in the application menu opens a local JSON file.
  The same channel format as the online catalog is required, for example:

      {
        "channels": [
          {
            "id": "my-channel",
            "name": "My TV",
            "url": "https://example.org/live.m3u8",
            "headers": {"Referer": "https://example.org/"}
          }
        ]
      }

- The selected local file is remembered in
  `~/.config/kanalik-gnome/catalog-source` (or `$XDG_CONFIG_HOME`)
  and re-read on startup and on **Reload channels**. Changes in the original
  JSON file therefore take effect without re-importing it.
- **Use default catalog** in the same menu switches back to the online JSON.
- Invalid/unreadable custom files are rejected without replacing the current
  channels; the error is shown in the main window.

## Diagnostic stream checker

Open the app menu and choose **Check stream availability…**.

- Checks three channels concurrently with headless **mpv**, using exactly
  the same stream URLs and HTTP headers.
- A successful sample = online. Timeout = **unknown**, not failed.
- Details and elapsed times are shown in the secondary window.
- **Copy Report** copies the full CSV report to the clipboard.
- **Export CSV…** opens a native Save dialog for the same report.
- Reports include every catalog channel in its original order, ID, name, URL, status,
  time to complete, local check timestamp, and diagnostic details. Incomplete
  checks are marked Pending or Not checked, never silently omitted.
- Checking does not interrupt TV playback.

## Install as a GNOME application

From the repository (currently the `feature/gnome-stream-checker` branch):

    git fetch origin feature/gnome-stream-checker
    git switch feature/gnome-stream-checker
    ./desktop/install.sh

The installer checks for missing dependencies and offers to install them
through `pacman` (Manjaro/Arch) or `apt` (Debian/Ubuntu). It builds the
release binary and installs **only for your user**, without needing `sudo`
except when system dependencies are missing:

- Binary: `~/.local/bin/kanalik-gnome`
- GNOME application launcher: `~/.local/share/applications/com.github.ziacik.Kanalik.desktop`

You can then launch **Kanálik** from GNOME's applications overview. Re-running
the installer updates the binary and launcher, without clearing channel
preferences, cached catalogs or the last selected channel.

Optional commands:

    ./desktop/install.sh --skip-deps           # don't install system packages
    ./desktop/install.sh --binary /path/to/kanalik-gnome  # install prebuilt binary
    ./desktop/install.sh --uninstall            # remove app, preserve user data

The installer uses `$XDG_DATA_HOME` for the launcher if defined, and
`$KANALIK_BIN_DIR` for the binary if explicitly set. GTK4 >= 4.12,
libadwaita >= 1.4, libmpv and Rust with Cargo are required for compiling.

### Run without installing

    cd desktop
    cargo run --release

The embedded player uses libmpv via GTK4's GLArea/OpenGL and EGL, not X11's
window ID, so it is suitable for Wayland.

## CI

    cargo fmt --all --check
    cargo test
    cargo clippy --all-targets -- -D warnings

EPG, archive and Sweet.tv resolvers are not yet ported to the desktop app.

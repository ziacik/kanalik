#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
APP_ID="com.github.ziacik.Kanalik"
BINARY_NAME="kanalik-gnome"
BIN_DIR="${KANALIK_BIN_DIR:-$HOME/.local/bin}"
APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
TARGET_BIN="$BIN_DIR/$BINARY_NAME"
TARGET_DESKTOP="$APPS_DIR/$APP_ID.desktop"
DESKTOP_TEMPLATE="$SCRIPT_DIR/packaging/$APP_ID.desktop"

usage() {
    cat <<'HELP'
Usage: bash desktop/install.sh [--skip-deps] [--binary PATH]
       bash desktop/install.sh --uninstall

Build Kanálik in release mode and install it for the current Linux user.
  --skip-deps     Do not try to install missing system dependencies
  --binary PATH   Install an existing executable instead of building
  --uninstall     Remove the installed binary and application launcher
  --help          Show this help
HELP
}

fail() {
    printf 'Error: %s\n' "$*" >&2
    exit 1
}

install_deps() {
    local -a missing=()
    local package

    if command -v pacman >/dev/null 2>&1; then
        for package in gtk4 libadwaita mpv libglvnd; do
            if ! pacman -Qq "$package" >/dev/null 2>&1; then
                missing+=("$package")
            fi
        done
        if ! command -v pkg-config >/dev/null 2>&1; then
            missing+=(pkgconf)
        fi
        if ! command -v cc >/dev/null 2>&1; then
            missing+=(base-devel)
        fi
        if ! command -v cargo >/dev/null 2>&1; then
            missing+=(rust)
        fi
        if ((${#missing[@]} > 0)); then
            command -v sudo >/dev/null 2>&1 || fail "sudo is required to install missing packages: ${missing[*]}"
            printf 'Installing missing system dependencies: %s\n' "${missing[*]}"
            sudo pacman -S --needed "${missing[@]}"
        fi
    elif command -v apt-get >/dev/null 2>&1; then
        for package in build-essential pkg-config libgtk-4-dev libadwaita-1-dev libmpv-dev mpv libegl1; do
            if ! dpkg-query -W -f='${Status}' "$package" 2>/dev/null | grep -q 'install ok installed'; then
                missing+=("$package")
            fi
        done
        if ! command -v cargo >/dev/null 2>&1; then
            missing+=(cargo rustc)
        fi
        if ((${#missing[@]} > 0)); then
            command -v sudo >/dev/null 2>&1 || fail "sudo is required to install missing packages: ${missing[*]}"
            printf 'Installing missing system dependencies: %s\n' "${missing[*]}"
            sudo apt-get update
            sudo apt-get install "${missing[@]}"
        fi
    else
        printf 'Unknown package manager: install GTK4, libadwaita, libmpv, OpenGL/EGL, pkg-config, C build tools and Rust manually.\n' >&2
    fi
}

check_build_deps() {
    command -v cargo >/dev/null 2>&1 || fail 'cargo is missing (install a current stable Rust toolchain)'
    command -v pkg-config >/dev/null 2>&1 || fail 'pkg-config is missing'
    pkg-config --atleast-version=4.12 gtk4 || fail 'GTK 4.12 or newer is required'
    pkg-config --atleast-version=1.4 libadwaita-1 || fail 'libadwaita 1.4 or newer is required'
    pkg-config --exists mpv || fail 'libmpv development package is required (mpv.pc missing)'
}

update_desktop_db() {
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$APPS_DIR" || true
    fi
}

uninstall_app() {
    rm -f -- "$TARGET_BIN" "$TARGET_DESKTOP"
    update_desktop_db
    printf 'Uninstalled Kanálik. User settings and cache were kept.\n'
}

skip_deps=false
binary=''
uninstall=false
while (($# > 0)); do
    case "$1" in
        --skip-deps) skip_deps=true ;;
        --binary)
            (($# >= 2)) || fail '--binary requires a path'
            binary="$2"
            shift
            ;;
        --uninstall) uninstall=true ;;
        -h|--help) usage; exit 0 ;;
        *) fail "Unknown argument: $1" ;;
    esac
    shift
done

if [[ "$uninstall" == true ]]; then
    uninstall_app
    exit 0
fi

[[ -f "$DESKTOP_TEMPLATE" ]] || fail "Missing launcher template: $DESKTOP_TEMPLATE"

if [[ -z "$binary" ]]; then
    if [[ "$skip_deps" == false ]]; then
        install_deps
    fi
    check_build_deps
    printf 'Building Kanálik (release)...\n'
    cargo build --release --manifest-path "$SCRIPT_DIR/Cargo.toml"
    binary="${CARGO_TARGET_DIR:-$SCRIPT_DIR/target}/release/$BINARY_NAME"
fi
[[ -f "$binary" && -x "$binary" ]] || fail "No executable found at $binary"

mkdir -p -- "$BIN_DIR" "$APPS_DIR"
tmp_bin="$(mktemp "$BIN_DIR/.kanalik-gnome.XXXXXX")"
tmp_desktop="$(mktemp "$APPS_DIR/.kanalik-desktop.XXXXXX")"
trap 'rm -f -- "$tmp_bin" "$tmp_desktop"' EXIT

install -m 0755 -- "$binary" "$tmp_bin"
mv -f -- "$tmp_bin" "$TARGET_BIN"

# Desktop Exec requires its executable path quoted and reserved characters escaped.
escaped_exec="$TARGET_BIN"
escaped_exec="${escaped_exec//\\/\\\\}"
escaped_exec="${escaped_exec//\"/\\\"}"
escaped_exec="${escaped_exec//\$/\\\$}"
backtick=$'\x60'
escaped_exec="${escaped_exec//"$backtick"/\\$backtick}"
while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" == Exec=* ]]; then
        printf 'Exec="%s"\n' "$escaped_exec"
    else
        printf '%s\n' "$line"
    fi
done < "$DESKTOP_TEMPLATE" > "$tmp_desktop"
chmod 0644 "$tmp_desktop"
mv -f -- "$tmp_desktop" "$TARGET_DESKTOP"
update_desktop_db

printf 'Installed Kanálik:\n  Binary: %s\n  Launcher: %s\n' "$TARGET_BIN" "$TARGET_DESKTOP"
printf 'Open Kanálik from your GNOME applications menu.\n'

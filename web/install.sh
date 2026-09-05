#!/usr/bin/env bash
#
# Sender server installer — https://github.com/mamounhisham1/sender
#
#   curl -fsSL <site>/install.sh | bash
#   curl -fsSL <site>/install.sh | bash -s -- --build-from-source
#
# Default: downloads a prebuilt binary from GitHub Releases (fast, no
# toolchain needed). --build-from-source clones the repo and compiles
# with cargo instead (needs Rust; installs it via rustup if missing).
#
set -euo pipefail

REPO="${SENDER_REPO:-https://github.com/mamounhisham1/sender.git}"
API="https://api.github.com/repos/mamounhisham1/sender/releases"
VERSION="${SENDER_VERSION:-latest}"
DEST="${SENDER_DIR:-$HOME/.local/share/sender}"
BIN_DIR="${SENDER_BIN_DIR:-$HOME/.local/bin}"
FROM_SOURCE=0

for arg in "$@"; do
  case "$arg" in
    --build-from-source) FROM_SOURCE=1 ;;
    -h|--help)
      echo "Usage: install.sh [--build-from-source]"
      echo "  Env: SENDER_VERSION=<tag|latest>  SENDER_BIN_DIR=<dir>  SENDER_DIR=<dir>"
      exit 0 ;;
    *) echo "Unknown option: $arg" >&2; exit 1 ;;
  esac
done

info()  { printf '\033[1;36m::\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m✓\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m!\033[0m %s\n' "$*" >&2; }
die()   { printf '\033[1;31m✗\033[0m %s\n' "$*" >&2; exit 1; }

have() { command -v "$1" >/dev/null 2>&1; }

# --- platform --------------------------------------------------------------
OS="$(uname -s)"; ARCH="$(uname -m)"
case "$OS-$ARCH" in
  Linux-x86_64)   TARGET="x86_64-unknown-linux-gnu" ;;
  Linux-aarch64|Linux-arm64) TARGET="aarch64-unknown-linux-gnu" ;;
  Darwin-arm64)   TARGET="aarch64-apple-darwin" ;;
  Darwin-x86_64)
    warn "Intel Mac detected — no prebuilt binary; falling back to source build."
    FROM_SOURCE=1; TARGET="" ;;
  *) die "Unsupported platform: $OS $ARCH (Linux x64/ARM64 and Apple Silicon only; otherwise use --build-from-source on a supported box)." ;;
esac

install_binary() {
  mkdir -p "$BIN_DIR"
  install -m 755 "$1" "$BIN_DIR/sender-server"
  ok "Installed $BIN_DIR/sender-server"
  case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) warn "$BIN_DIR is not on your PATH. Add it:  export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
  esac
}

print_next_steps() {
  echo
  ok "Done! Start the server with:"
  echo "    sender-server"
  echo
  echo "Then on your phone: open the Sender app in Expo Go and tap"
  echo "“📷 Scan laptop QR” pointing at the QR in the server window."
  echo "Both devices must be on the same Wi-Fi."
}

# --- fast path: prebuilt binary --------------------------------------------
if [ "$FROM_SOURCE" = 0 ]; then
  TAG="$VERSION"
  if [ "$TAG" = "latest" ]; then
    info "Finding latest release…"
    TAG="$(curl -fsSL "$API/latest" | grep -o '"tag_name": *"[^"]*"' | head -1 | cut -d'"' -f4)"
    [ -n "$TAG" ] || die "Could not determine latest release. Is the repo public with a release published?"
  fi
  URL="https://github.com/mamounhisham1/sender/releases/download/$TAG/sender-server-$TARGET.tar.gz"
  info "Downloading sender-server $TAG ($TARGET)…"
  TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
  if curl -fsSL "$URL" -o "$TMP/sender.tar.gz"; then
    tar -xzf "$TMP/sender.tar.gz" -C "$TMP"
    install_binary "$TMP/sender-server"
    print_next_steps
    exit 0
  fi
  die "No prebuilt binary for $TARGET at $TAG. Re-run with --build-from-source."
fi

# --- slow path: build from source -------------------------------------------
case "$OS" in
  Linux|Darwin) ;;
  *) die "Unsupported OS: $OS (Linux and macOS only; on Windows use WSL2)." ;;
esac

if [ "$OS" = "Linux" ]; then
  if ! have cc && ! have gcc && ! have clang; then
    info "Installing a C compiler + clipboard tools (needs sudo)…"
    if have apt-get; then
      sudo apt-get update -y && sudo apt-get install -y git curl build-essential wl-clipboard xclip \
        || warn "apt install failed — build may still work if a compiler exists."
    elif have dnf; then
      sudo dnf install -y git curl gcc wl-clipboard xclip \
        || warn "dnf install failed — continuing anyway."
    elif have pacman; then
      sudo pacman -Sy --noconfirm --needed git curl base-devel wl-clipboard xclip \
        || warn "pacman install failed — continuing anyway."
    else
      warn "No known package manager — make sure git, curl and a C compiler are installed."
    fi
  fi
fi
for dep in git curl; do
  have "$dep" || die "Missing required tool: $dep"
done
if [ "$OS" = "Darwin" ]; then
  xcode-select -p >/dev/null 2>&1 || warn "Xcode Command Line Tools not found — run: xcode-select --install"
fi

if ! have cargo; then
  info "Rust not found — installing via rustup…"
  curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi
have cargo || die "cargo still not on PATH after rustup. Restart your shell and re-run."

if [ -d "$DEST/.git" ]; then
  info "Updating existing checkout at $DEST…"
  git -C "$DEST" pull --ff-only || warn "git pull failed — building from existing checkout."
else
  info "Cloning sender to $DEST…"
  rm -rf "$DEST"
  git clone --depth 1 "$REPO" "$DEST" || die "git clone failed. Is the repo public and the network up?"
fi

info "Building sender-server (release, first build takes a few minutes)…"
cargo build --release --manifest-path "$DEST/server/Cargo.toml"

install_binary "$DEST/server/target/release/sender-server"
print_next_steps

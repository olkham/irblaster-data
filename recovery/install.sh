#!/usr/bin/env bash
# install.sh - build IR Blaster Recovery from source into recovery/dist/ (Linux and macOS).
#
#   ./install.sh           build or rebuild the app; safe to run again
#   ./install.sh --clean   delete earlier builds (target/ and dist/) and build from scratch
#   ./install.sh --help
#
# Then start it with ./run.sh. Windows: install.bat. Without Rust installed: docker compose (see README.md).
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"
MIN_RUST="1.95"

usage() { sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'; }
die() { echo "error: $*" >&2; exit 1; }

CLEAN=0
for arg in "$@"; do
  case "$arg" in
    --clean) CLEAN=1 ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option \"$arg\" (see --help)" ;;
  esac
done

# version_lt A B: true when version A is older than B
version_lt() { [ "$1" != "$2" ] && [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | head -1)" = "$1" ]; }

# --- Rust -------------------------------------------------------------------------------
[ -x "$HOME/.cargo/bin/cargo" ] && export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust is not installed. It is needed to build the app (about 1 GB of downloads)."
  read -r -p "Install it now with rustup from https://rustup.rs? [y/N] " answer
  case "$answer" in
    y|Y|yes|YES)
      command -v curl >/dev/null 2>&1 || die "curl is needed to install Rust"
      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
      export PATH="$HOME/.cargo/bin:$PATH" ;;
    *) die "install Rust from https://rustup.rs, then run ./install.sh again" ;;
  esac
fi
RUST_VERSION="$(rustc --version | awk '{print $2}')"
if version_lt "$RUST_VERSION" "$MIN_RUST"; then
  if command -v rustup >/dev/null 2>&1; then
    echo "==> Rust $RUST_VERSION is older than $MIN_RUST, updating"
    rustup update stable
  else
    die "Rust $MIN_RUST or newer is needed (found $RUST_VERSION)"
  fi
fi

# --- C linker ---------------------------------------------------------------------------
if ! command -v cc >/dev/null 2>&1; then
  case "$(uname -s)" in
    Darwin) die "a C linker is needed: run  xcode-select --install  and try again" ;;
    *) die "a C compiler and linker are needed: e.g.  sudo apt install build-essential  (Debian/Ubuntu) or  sudo dnf install gcc  (Fedora)" ;;
  esac
fi

# --- build ------------------------------------------------------------------------------
if [ "$CLEAN" = 1 ]; then
  echo "==> Removing earlier builds"
  cargo clean
  rm -rf dist
fi

echo "==> Building (the first build downloads and compiles dependencies, a few minutes)"
cargo build --release --locked
mkdir -p dist
cp target/release/irblaster-recovery dist/irblaster-recovery
echo "==> Built dist/irblaster-recovery ($(dist/irblaster-recovery --version))"

if [ "$(uname -s)" = Linux ] && ! id -nG | grep -qwE 'dialout|uucp'; then
  echo
  echo "note: to use USB serial ports, add yourself to the serial-port group and log in again:"
  echo "      sudo usermod -aG dialout \$USER     (uucp on Arch and Fedora)"
fi
echo
echo "Start it with ./run.sh (or ./run.sh --help for the command line)."

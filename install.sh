#!/usr/bin/env bash
set -euo pipefail

################################################################################
# dd-rs installer — one-liner to get dd-rs on your system
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/dd-rs/dd-rs/main/install.sh | bash
#
# What it does:
#   1. Checks for required dependencies (rust/cargo, cc, git, make)
#   2. Clones the dd-rs repository
#   3. Builds the release binary
#   4. Installs to /usr/local/bin
#   5. Cleans up
################################################################################

REPO_URL="https://github.com/dd-rs/dd-rs.git"
INSTALL_DIR="/usr/local/bin"
BINARY_NAME="dd-rs"
TMP_DIR=""

# ---------------------------------------------------------------------------
# Colors
# ---------------------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m' # No Color

info()  { printf "${BLUE}→${NC} %s\n" "$*"; }
ok()    { printf "${GREEN}✓${NC} %s\n" "$*"; }
warn()  { printf "${YELLOW}⚠${NC} %s\n" "$*"; }
err()   { printf "${RED}✗${NC} %s\n" "$*"; }
header(){ printf "\n${BOLD}%s${NC}\n" "$*"; }

# ---------------------------------------------------------------------------
# Cleanup
# ---------------------------------------------------------------------------
cleanup() {
    if [ -n "$TMP_DIR" ] && [ -d "$TMP_DIR" ]; then
        rm -rf "$TMP_DIR"
    fi
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Header
# ---------------------------------------------------------------------------
echo ""
echo "  ╔══════════════════════════════════════════════════════════╗"
echo "  ║              dd-rs — Safe Modern dd                      ║"
echo "  ║              One-liner Installer                         ║"
echo "  ╚══════════════════════════════════════════════════════════╝"
echo ""

# ---------------------------------------------------------------------------
# Step 1: Check dependencies
# ---------------------------------------------------------------------------
header "Checking dependencies..."

# Rust / Cargo
if command -v cargo &>/dev/null; then
    ok "cargo $(cargo --version 2>/dev/null | head -1 | awk '{print $2}')"
elif command -v rustup &>/dev/null; then
    warn "cargo not in PATH, but rustup found. Trying to source..."
    if [ -f "$HOME/.cargo/env" ]; then
        source "$HOME/.cargo/env"
    fi
    export PATH="$HOME/.cargo/bin:$PATH"
    if command -v cargo &>/dev/null; then
        ok "cargo $(cargo --version 2>/dev/null | head -1 | awk '{print $2}') (sourced)"
    else
        err "cargo not found after sourcing rustup."
        echo ""
        echo "  Install Rust from: https://rustup.rs"
        echo "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    fi
else
    err "Rust is not installed."
    echo ""
    echo "  Install Rust from: https://rustup.rs"
    echo "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

# C compiler
CC="${CC:-cc}"
if command -v "$CC" &>/dev/null; then
    ok "$CC compiler found"
elif command -v gcc &>/dev/null; then
    export CC=gcc
    ok "gcc $(gcc --version 2>/dev/null | head -1 | awk '{print $3}')"
elif command -v clang &>/dev/null; then
    export CC=clang
    ok "clang $(clang --version 2>/dev/null | head -1 | awk '{print $3}')"
else
    err "No C compiler found. Install gcc or clang."
    echo ""
    echo "  macOS:  xcode-select --install"
    echo "  Linux:  sudo apt install build-essential    (Debian/Ubuntu)"
    echo "          sudo dnf install gcc make           (Fedora)"
    echo "          sudo pacman -S gcc make             (Arch)"
    exit 1
fi

# Git
if command -v git &>/dev/null; then
    ok "git $(git --version 2>/dev/null | awk '{print $3}')"
else
    warn "git not found — will download tarball instead"
fi

# ---------------------------------------------------------------------------
# Step 2: Download source
# ---------------------------------------------------------------------------
header "Downloading dd-rs..."

TMP_DIR=$(mktemp -d 2>/dev/null || mktemp -d -t 'ddrs-install')
cd "$TMP_DIR"

if command -v git &>/dev/null; then
    info "Cloning $REPO_URL ..."
    git clone --depth 1 "$REPO_URL" . 2>/dev/null || {
        err "Failed to clone repository. Check your internet connection."
        exit 1
    }
    ok "Repository cloned"
else
    # Fallback: download tarball
    TARBALL_URL="https://github.com/dd-rs/dd-rs/archive/refs/heads/main.tar.gz"
    info "Downloading $TARBALL_URL ..."
    if command -v curl &>/dev/null; then
        curl -fsSL "$TARBALL_URL" -o dd-rs.tar.gz
    elif command -v wget &>/dev/null; then
        wget -q "$TARBALL_URL" -O dd-rs.tar.gz
    else
        err "Neither curl nor wget found. Install one to continue."
        exit 1
    fi
    tar xzf dd-rs.tar.gz --strip-components=1
    ok "Tarball extracted"
fi

# ---------------------------------------------------------------------------
# Step 3: Build
# ---------------------------------------------------------------------------
header "Building dd-rs (release profile)..."

cargo build --release 2>&1 | while IFS= read -r line; do
    case "$line" in
        *error*) err "$line" ;;
        *warning*) warn "$line" ;;
        *Compiling*|*Building*|*Finished*) info "$line" ;;
    esac
done

BUILD_EXIT_CODE=${PIPESTATUS[0]}
if [ "$BUILD_EXIT_CODE" -ne 0 ]; then
    err "Build failed (exit code $BUILD_EXIT_CODE)."
    exit 1
fi

if [ ! -f "target/release/$BINARY_NAME" ]; then
    err "Binary not found at target/release/$BINARY_NAME after build."
    exit 1
fi

ok "Build complete — $(ls -lh "target/release/$BINARY_NAME" | awk '{print $5}')"

# ---------------------------------------------------------------------------
# Step 4: Install
# ---------------------------------------------------------------------------
header "Installing to $INSTALL_DIR/$BINARY_NAME ..."

if [ -f "$INSTALL_DIR/$BINARY_NAME" ]; then
    warn "$INSTALL_DIR/$BINARY_NAME already exists — replacing"
fi

if [ -w "$INSTALL_DIR" ]; then
    cp "target/release/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
else
    info "Need sudo to write to $INSTALL_DIR"
    sudo cp "target/release/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
fi

chmod +x "$INSTALL_DIR/$BINARY_NAME" 2>/dev/null || true

ok "Installed to $INSTALL_DIR/$BINARY_NAME"

# ---------------------------------------------------------------------------
# Step 5: Verify
# ---------------------------------------------------------------------------
header "Verifying installation..."

if command -v "$BINARY_NAME" &>/dev/null; then
    VERSION=$("$BINARY_NAME" --version 2>&1 | head -1)
    ok "$VERSION"
else
    warn "$BINARY_NAME not in PATH yet — you may need to restart your shell"
    info "Binary is at: $INSTALL_DIR/$BINARY_NAME"
fi

# Quick smoke test
info "Running smoke test..."
if "$INSTALL_DIR/$BINARY_NAME" if=/dev/zero of=/dev/null bs=1K count=1 status=none 2>/dev/null; then
    ok "Smoke test passed"
else
    warn "Smoke test had issues — binary may still work, try it manually"
fi

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------
echo ""
echo "  ╔══════════════════════════════════════════════════════════╗"
echo "  ║  ✓  dd-rs installed successfully!                       ║"
echo "  ║                                                        ║"
echo "  ║  Try it out:                                           ║"
echo "  ║    dd-rs --help                                        ║"
echo "  ║    dd-rs if=/dev/zero of=test.bin bs=1M count=10       ║"
echo "  ║    dd-rs explain if=/dev/zero of=/dev/sda              ║"
echo "  ║    dd-rs info /dev/sda                                 ║"
echo "  ╚══════════════════════════════════════════════════════════╝"
echo ""

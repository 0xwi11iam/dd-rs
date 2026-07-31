#!/usr/bin/env bash
set -euo pipefail
# dd-rs installer — curl to install
# curl -fsSL https://raw.githubusercontent.com/0xwi11iam/dd-rs/main/install.sh | bash

BINARY="dd-rs"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
REPO="https://github.com/0xwi11iam/dd-rs.git"
TMP_DIR=""

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; BOLD='\033[1m'; NC='\033[0m'
info()  { printf "  ${CYAN}→${NC} %s\n" "$*"; }
ok()    { printf "  ${GREEN}✓${NC} %s\n" "$*"; }
warn()  { printf "  ${YELLOW}⚠${NC} %s\n" "$*"; }
err()   { printf "  ${RED}✗${NC} %s\n" "$*"; exit 1; }

cleanup() { [ -n "$TMP_DIR" ] && [ -d "$TMP_DIR" ] && rm -rf "$TMP_DIR"; }
trap cleanup EXIT

echo ""
echo "  ${BOLD}dd-rs — installer${NC}"
echo ""

# ── Check deps ──
info "Checking dependencies..."

command -v cargo &>/dev/null || {
    if [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
    export PATH="$HOME/.cargo/bin:$PATH"
}
command -v cargo &>/dev/null || err "Rust not found. Install: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
ok "cargo $(cargo --version | awk '{print $2}')"

command -v cc &>/dev/null || command -v gcc &>/dev/null || command -v clang &>/dev/null || \
    err "No C compiler. macOS: xcode-select --install  |  Linux: apt install build-essential"
ok "C compiler found"

# ── Try cargo install first (fastest) ──
info "Trying cargo install..."
if cargo install "$BINARY" 2>/dev/null; then
    ok "Installed via cargo"
    echo ""
    echo "  Try it: ${BOLD}${BINARY} --help${NC}"
    echo "          ${BOLD}${BINARY} if=/dev/zero of=test.bin bs=1M count=10${NC}"
    exit 0
fi
warn "cargo install failed — building from source"

# ── Clone and build ──
TMP_DIR=$(mktemp -d)
cd "$TMP_DIR"

if command -v git &>/dev/null; then
    git clone --depth 1 "$REPO" . 2>/dev/null || err "Failed to clone $REPO"
else
    curl -fsSL "${REPO%.git}/archive/refs/heads/main.tar.gz" | tar xz --strip-components=1 || \
        err "Failed to download source"
fi
ok "Source downloaded"

info "Building (release)..."
cargo build --release 2>&1 | grep -E 'Compiling|Finished' || true
[ -f "target/release/$BINARY" ] || err "Build failed — binary not found"
ok "Build complete"

# ── Install ──
info "Installing to $INSTALL_DIR/$BINARY"
[ -w "$INSTALL_DIR" ] || { info "Need sudo..."; sudo cp "target/release/$BINARY" "$INSTALL_DIR/$BINARY"; }
[ -w "$INSTALL_DIR" ] && cp "target/release/$BINARY" "$INSTALL_DIR/$BINARY"
chmod +x "$INSTALL_DIR/$BINARY" 2>/dev/null || true
ok "Installed"

# ── Verify ──
"$INSTALL_DIR/$BINARY" --version 2>&1 | head -1
"$INSTALL_DIR/$BINARY" if=/dev/zero of=/dev/null bs=1K count=1 status=none 2>/dev/null && ok "Smoke test passed"

echo ""
echo "  ${GREEN}✓ dd-rs is ready.${NC}"
echo "  Try: ${BOLD}dd-rs --help${NC}"
echo "       ${BOLD}dd-rs explain if=/dev/zero of=/dev/sda${NC}"

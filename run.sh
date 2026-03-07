#!/usr/bin/env bash
set -euo pipefail

# Check for Rust toolchain
if ! command -v cargo &> /dev/null; then
    echo "Error: Rust toolchain not found. Install via: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

# Check for system dependencies
for lib in hidapi libusb-1.0; do
    if ! pkg-config --exists "$lib" 2>/dev/null; then
        echo "Error: $lib not found. Install via: sudo pacman -S hidapi libusb"
        exit 1
    fi
done

cargo build --release
echo ""
echo "Built successfully."
echo ""
echo "Binaries:"
echo "  ./target/release/unifand      — daemon (owns hardware, streams video)"
echo "  ./target/release/unifanctl    — CLI control tool"
echo ""
echo "Quick start:"
echo "  ./target/release/unifand                              # start daemon"
echo "  ./target/release/unifanctl status                     # check daemon"
echo "  ./target/release/unifanctl display video foo.webm     # stream video"
echo ""
echo "Install as systemd user service:"
echo "  cp systemd/unifand.service ~/.config/systemd/user/"
echo "  systemctl --user daemon-reload"
echo "  systemctl --user enable --now unifand"

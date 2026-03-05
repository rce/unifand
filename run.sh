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
echo "Built successfully. Run with:"
echo "  ./target/release/unifanctl discover"
echo "  ./target/release/unifanctl fan status"
echo "  ./target/release/unifanctl --help"

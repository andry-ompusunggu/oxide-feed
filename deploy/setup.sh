#!/usr/bin/env bash
set -euo pipefail

# ═══════════════════════════════════════════════
# OxideFeed — Production Setup Script
# ═══════════════════════════════════════════════
# Run this script on your laptop/server to install
# OxideFeed as a systemd service with auto-start.
#
# Usage:
#   sudo ./setup.sh
# ═══════════════════════════════════════════════

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo "🔧 OxideFeed Production Setup"
echo "=============================="
echo ""

# --- Step 1: Build release binary ---
echo "[1/5] Building release binary..."
cd "$PROJECT_DIR"
cargo build --release --quiet
echo "  ✅ Build complete"

# --- Step 2: Create production directory ---
echo "[2/5] Creating production directory..."
mkdir -p /opt/oxide-feed/bin
mkdir -p /opt/oxide-feed/logs
echo "  ✅ /opt/oxide-feed/ created"

# --- Step 3: Copy binary & config ---
echo "[3/5] Copying files..."
cp target/release/oxide-feed /opt/oxide-feed/bin/
strip /opt/oxide-feed/bin/oxide-feed
echo "  ✅ Binary copied (size: $(ls -lh /opt/oxide-feed/bin/oxide-feed | awk '{print $5}'))"

if [ -f .env ]; then
    cp .env /opt/oxide-feed/.env
    echo "  ✅ .env copied"
    echo "  ⚠️  Jangan lupa edit credentials di /opt/oxide-feed/.env!"
else
    echo "  ⚠️  .env tidak ditemukan! Buat dulu: cp .env.example /opt/oxide-feed/.env"
fi

# --- Step 4: Install systemd service ---
echo "[4/5] Installing systemd service..."
cp deploy/oxide-feed.service /etc/systemd/system/oxide-feed.service
chmod 644 /etc/systemd/system/oxide-feed.service
systemctl daemon-reload
systemctl enable oxide-feed
echo "  ✅ Service installed & enabled"

# --- Step 5: Start service ---
echo "[5/5] Starting OxideFeed..."
systemctl start oxide-feed

# --- Verify ---
sleep 2
if systemctl is-active --quiet oxide-feed; then
    echo ""
    echo "✅ OxideFeed is RUNNING!"
    echo "   📍 Binary:  /opt/oxide-feed/bin/oxide-feed"
    echo "   📍 Config:  /opt/oxide-feed/.env"
    echo "   📍 DB:      /opt/oxide-feed/oxide_feed.db"
    echo "   📍 Logs:    journalctl -u oxide-feed -f"
    echo ""
    echo "   🔄 Auto-start on boot: ENABLED"
else
    echo "❌ OxideFeed failed to start!"
    echo "   Check logs: journalctl -u oxide-feed -n 50 --no-pager"
    exit 1
fi

echo ""
echo "🎉 Setup complete! OxideFeed akan auto-start setiap laptop dinyalakan."
echo "   📊 Cek log: sudo journalctl -u oxide-feed -f"

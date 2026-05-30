#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LABEL="com.cuong.cc-autoresume"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
BIN="$HOME/.local/bin/cc-autoresume"
LOG="$HOME/.claude/auto-resume/watch.log"

# 1. Supersede any running daemon (Python or old Rust)
launchctl unload "$PLIST" 2>/dev/null || true

# 2. Build release binary
cd "$ROOT" && cargo build --release
mkdir -p "$HOME/.local/bin" "$HOME/.claude/auto-resume"
rm -f "$BIN"   # drop any prior symlink so cp writes a fresh file (not through it)
cp "$ROOT/target/release/cc-autoresume" "$BIN"

# 3. Install + load LaunchAgent
sed -e "s#__BIN__#$BIN#g" -e "s#__LOG__#$LOG#g" "$ROOT/launchd/$LABEL.plist" > "$PLIST"
launchctl load "$PLIST"

echo "Installed. cc-autoresume (Rust) is on ~/.local/bin and the watcher + dashboard are running."
echo "Open the dashboard:  cc-autoresume url   (LAN + token; QR for your phone is in Settings)"
echo "Optional sleep-wake (wake the Mac at reset time): add to sudoers via 'sudo visudo':"
echo "  $(whoami) ALL=(root) NOPASSWD: /usr/bin/pmset schedule wake *"

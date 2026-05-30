# cc-autoresume (Rust)

Single-binary daemon that auto-resumes Claude Code when your usage limit resets,
with a web dashboard.

## Build & install
    cargo build --release
    ./install.sh

## CLI
    cc-autoresume mode auto|ask|off
    cc-autoresume msg "..."
    cc-autoresume list | status
    cc-autoresume cancel [prefix] | arm <prefix> | fire <id>
    cc-autoresume url | token [--rotate]

State: ~/.claude/auto-resume/{config.json, pending/*.json}

## Dashboard
The `watch` daemon serves a web dashboard on `0.0.0.0:7317` (LAN + token). Open it with the tokenized URL:

    cc-autoresume url      # prints http://<lan-ip>:7317/?token=...

Features: light/dark theme, EN/VI, live countdowns, mode / force-headless / resume-message control, per-session resume message, cancel / arm / resume-now, conversation live-tail, and a Settings panel (URL + QR for your phone, token rotate). Scan the QR in Settings to open it on your phone.

# cc-autoresume (Rust)

Single-binary daemon that auto-resumes Claude Code when your usage limit resets.
Rust port of the original; web dashboard lands in Phase 2-3.

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

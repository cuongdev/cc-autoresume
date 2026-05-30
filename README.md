<div align="center">
  <img src="docs/logo.svg" width="96" alt="cc-autoresume"/>
  <h1>cc-autoresume</h1>
  <p><b>Auto-resume Claude Code when your usage limit resets — with a live web dashboard.</b></p>
</div>

## Why

When a Claude Code session hits its usage limit ("You've hit your limit · resets 2:30am"), it stops until you manually resume — even if the quota comes back while you're away. **cc-autoresume** watches for that on disk and, when the quota resets, resumes the session headless (`claude -p "<message>" --resume <id>`) so work continues unattended. A single static Rust binary — no Python, no venv.

## Features

- **Auto-resume on reset** — detects the limit, parses the reset time, resumes headless when quota returns (with backoff retry).
- **Modes** — `auto` (resume automatically), `ask` (wait for your confirm), `off` — global, per-project, or per-session.
- **Web dashboard** (LAN + token) — live state via SSE, countdown rings, mode/force-headless/message controls.
- **Active sessions** — see sessions active in the last 24h and **pre-configure** their resume message + mode *before* they hit a limit.
- **Conversation live-tail** — watch a headless resume happen in real time from the browser.
- **Per-session resume message** — each session can carry its own task-specific resume instruction.
- **Settings** — copyable URL, scannable **QR** to open on your phone, token rotation.
- **Polish** — light/dark theme, English/Vietnamese, inline help tooltips.

## Screenshot

![cc-autoresume dashboard](docs/screenshot.png)

## Install (macOS)

```bash
git clone git@github.com:cuongdev/cc-autoresume.git
cd cc-autoresume
cargo build --release
./install.sh        # builds, installs to ~/.local/bin, loads the LaunchAgent
```

`install.sh` runs the watcher + dashboard at login. Open the dashboard:

```bash
cc-autoresume dashboard      # prints http://<lan-ip>:7317/?token=...
```

## CLI

| Command | What |
|---|---|
| `cc-autoresume dashboard` | print the tokenized dashboard URL |
| `cc-autoresume mode auto\|ask\|off` | set the global mode |
| `cc-autoresume msg "<text>"` | set the global resume message |
| `cc-autoresume list` / `status` | list pending resumes |
| `cc-autoresume cancel [prefix]` | cancel one (by id prefix) or all |
| `cc-autoresume arm <prefix>` | confirm an ask-mode resume |
| `cc-autoresume fire <id>` | resume now |
| `cc-autoresume token [--rotate]` | print / rotate the dashboard token |
| `cc-autoresume watch` | run the daemon + dashboard (used by the LaunchAgent) |

## Dashboard

Served by the `watch` daemon on `0.0.0.0:7317`. Auth is a bearer token (also accepted as `?token=` for SSE). Open `cc-autoresume dashboard` on your Mac, or scan the QR in **⚙ Settings** to open it on your phone (same LAN). From the dashboard you can: switch mode, edit the resume message (global or per-session), force-headless, cancel/arm/resume-now, browse active sessions, and watch a session's conversation live.

## How it works

```
~/.claude/projects/**/*.jsonl   (Claude Code transcripts)
        │  tail (watcher, 20s)
        ▼
   detect "limit · resets <time>"  →  parse reset time  →  arm a pending resume
        │                                   (preset → per-project → global)
        ▼  at reset (pmset wake best-effort)
   liveness check (lsof) → claude -p "<msg>" --resume <id>  → backoff if still limited
```

State lives in `~/.claude/auto-resume/`: `config.json`, `pending/<id>.json`, `stats.json`, `sessions.json`.

## Configuration

`~/.claude/auto-resume/config.json` (camelCase): `mode`, `defaultMessage`, `forceHeadless`, `backoff {everySec, maxAttempts}`, `perProject`, `port` (default 7317), `token` (auto-generated). Per-session presets live in `sessions.json`.

## Security

The dashboard binds `0.0.0.0` (so your phone on the same LAN can reach it) and is guarded by a random token over plain HTTP — anyone with the token on your network can control resumes (which run `claude`). Use on a trusted LAN; rotate the token from Settings if needed. No TLS (out of scope).

## Limitations

- macOS-first (LaunchAgent + `pmset` + osascript notifications). Linux would need systemd + `rtcwake` + `notify-send`.
- Waking a *sleeping* Mac at reset needs an optional sudoers entry (printed by `install.sh`); otherwise resume fires on the next wake.
- The official Claude usage % is server-side and not shown.

## Development

```bash
cargo test          # unit + integration tests
cargo clippy --all-targets
cargo build --release
```

## License

MIT

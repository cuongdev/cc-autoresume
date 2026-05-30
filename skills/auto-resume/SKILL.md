---
name: auto-resume
description: Control cc-autoresume — the daemon that auto-continues a Claude Code session after its usage limit resets. Turn auto-resume on/off, set the resume message, open the dashboard, or view/cancel pending resumes. Trigger: /auto-resume
---

# auto-resume

`cc-autoresume` auto-resumes a Claude Code session when its usage limit resets.
The `watch` daemon also serves a web dashboard.

| Intent | Command |
|---|---|
| Open the dashboard (tokenized URL) | `cc-autoresume url` |
| Always auto-continue | `cc-autoresume mode auto` |
| Ask first (opt-in) | `cc-autoresume mode ask` |
| Never | `cc-autoresume mode off` |
| Set global resume message | `cc-autoresume msg "<text>"` |
| List armed resumes | `cc-autoresume list` |
| Cancel (by id prefix) / all | `cc-autoresume cancel <prefix>` / `cc-autoresume cancel` |
| Confirm an ask-mode resume | `cc-autoresume arm <prefix>` |
| Rotate the dashboard token | `cc-autoresume token --rotate` |

After running, report the resulting state to the user. Default mode is `auto`.

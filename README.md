# Luminus CLI

Terminal-native AI coding agent written in Rust. Ratatui TUI, streaming chat, provider switching, sessions, and permission-gated tools — similar in spirit to Hermes-style agent CLIs, still early and honest about what works.

**Status:** active development (`v0.1.0`). Offline `FakeProvider` works with no API key. OpenAI-compatible HTTP is opt-in via env vars. This is **not** a production-complete agent runtime.

## What works now

- **TUI** — Ratatui + Crossterm, streaming tokens, monochrome via `NO_COLOR`
- **Providers** — deterministic offline `fake`; OpenAI-compatible chat (SSE stream + JSON fallback) over Rustls
- **`/provider`** — show or switch (`fake` / `openai`)
- **`/discover`** — list models from the active provider’s `GET /models`
- **Models** — role map (`default` / `fast` / `deep`), `/model`, `/models`, `Ctrl+M` selector
- **Sessions** — JSON under the platform data dir; `/save`, `/sessions`, `/load` (atomic write, sanitized names); event log for tools/approvals
- **Skills** — `/skills`, `/skills list`, `/skills inspect <name>`, `/skill <name>` (built-in, global `~/.config/luminus/skills/`, project `.luminus/skills/`)
- **Diff & History** — `/diff` (interactive TUI overlay), `/changes`, `/undo`, `/redo`, `/revert-file <path>`
- **Tools** — `/tools`, `/tool <name> …` with approval overlay; specs: `read_file`, `write_file`, `list_dir`, `run_shell`, `file_meta`/`file_metadata`, `glob`, `grep`, `edit_file` (optional content-hash + unified diff output), `http_get` (disabled)
- **Approval choices** — `Y`/Enter once, `A` session allow, `P` project allow (persist), `N`/Esc reject, `D` session deny, `X` project deny (persist)
- **Security basics** — explicit approval; path canonicalization + project-root check for relative paths; sensitive-path deny (`.env`, keys, `.ssh`/`.aws`, …); shell denylist for a few destructive patterns; project policy file `.luminus/tool_policy.json`; API keys redacted in debug paths
- **Shell** — timeout (default 30s, `LUMINUS_SHELL_TIMEOUT_SECS`); background worker + `Esc`/`Ctrl+C` cancel via `CancellationToken`
- **Bounded output** — tool transcripts capped; truncated full text saved under `<data_root>/artifacts/` with `artifact_id`
- **Agents** — `/spawn <prompt>` one isolated child request; single-child policy; `Esc` / `Ctrl+C` cancellation; context-window estimate

## Limitations (honest)

- **`http_get` is disabled** — always denied / network off in this phase
- **`run_shell` is not sandboxed** — denylist is defense-in-depth only; approved commands run on the host shell
- **Absolute paths** can target outside the project after approval (relative paths are contained)
- **No full MCP / skills runtime / LSP / plugins** in the binary yet (a `skills/` tree may exist in the repo as reference material; the agent does not load it as a product feature)
- **One child agent / one tool** at a time; no full multi-tool parallel orchestration yet
- **No full pre-accept diff UI** (`/diff`/`/undo`) yet — `edit_file` still prints a post-apply unified diff + content fingerprint
- **Content hash** is a non-crypto `dh64:` DefaultHasher fingerprint (stale-edit guard), not SHA-256
- **Model catalog** is in-memory / role-based
- **No claim** of full Intruksi / production acceptance criteria

## Requirements

- Rust stable (edition **2024**; Rust **1.85+** recommended)
- Cargo, Unicode-capable terminal

Install Rust: https://rustup.rs/

## Build & run

```bash
git clone https://github.com/VanZPro/Luminus-CLI.git
cd Luminus-CLI
cargo build --release
cargo run            # debug
# or:
./target/release/luminus      # Unix
.\target\release\luminus.exe  # Windows
```

```bash
luminus --help
luminus --version
```

### Windows (optional PATH copy)

```powershell
winget install Rustlang.Rustup
# restart shell, then clone + cargo build --release
Copy-Item target\release\luminus.exe "$HOME\.local\bin\"
```

### Linux deps (Debian/Ubuntu)

```bash
sudo apt update && sudo apt install -y build-essential pkg-config libssl-dev git curl
```

## Configuration

Secrets are never committed. Use placeholders only:

| Variable | Purpose |
|---|---|
| `OPENAI_API_KEY` or `LUMINUS_OPENAI_API_KEY` | API key (required for `openai` provider) |
| `OPENAI_BASE_URL` or `LUMINUS_OPENAI_BASE_URL` | Base URL (default `https://api.openai.com/v1`) |
| `OPENAI_MODEL` or `LUMINUS_OPENAI_MODEL` | Model id (default `gpt-4o-mini`) |
| `LUMINUS_DATA_DIR` | Override session root |
| `LUMINUS_SHELL_TIMEOUT_SECS` | Shell timeout in seconds (default `30`) |
| `NO_COLOR` | Monochrome TUI if set |

```bash
export OPENAI_API_KEY='[REDACTED]'
export OPENAI_BASE_URL='https://api.openai.com/v1'   # optional
export OPENAI_MODEL='gpt-4o-mini'                    # optional
luminus
```

```powershell
$env:OPENAI_API_KEY = '[REDACTED]'
$env:OPENAI_BASE_URL = 'https://api.openai.com/v1'
$env:OPENAI_MODEL = 'gpt-4o-mini'
luminus
```

No key → offline fake provider. Invalid base URL with a key set → configuration error (no silent fallback).

Session files: `%LOCALAPPDATA%\luminus\sessions\` (Windows) or `~/.local/share/luminus/sessions/` (Unix), unless `LUMINUS_DATA_DIR` is set.

## Slash commands & keys

| Input | Action |
|---|---|
| `/help` | Command help |
| `/about` | App info |
| `/clear` | Clear in-memory conversation / agent UI state |
| `/exit` | Quit |
| `/model <role>` | Select role (`default`, `fast`, `deep`) |
| `/models` | List role → model map |
| `/discover` | Provider model discovery |
| `/save <name>` | Save conversation |
| `/sessions` | List saved sessions |
| `/load <name>` | Restore session |
| `/tools` | List tools |
| `/tool <name> <args...>` | Queue tool; needs approval |
| `/provider` | Show providers |
| `/provider fake` \| `openai` | Switch provider |
| `/spawn <prompt>` | Start one child-agent request |
| `Ctrl+M` | Model selector |
| `Esc` | Cancel child, then main request |
| `Ctrl+C` | Cancel main request; again when idle → exit |
| Approval: `y` / Enter · `n` / Esc | Approve or reject tool |

### Tool examples

```text
/tool read_file Cargo.toml
/tool list_dir .
/tool write_file notes.txt hello
/tool run_shell cargo test
```

## Development

```bash
cargo fmt -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release
```

`target/` and local planning docs are gitignored. Do not commit API keys or `.env`.

## Roadmap

- Stronger tool sandboxing / network policy
- Multi-agent beyond single-child
- MCP, skills loading, plugins
- LSP / code intelligence integration
- Packaged releases + CI artifacts

## License

MIT

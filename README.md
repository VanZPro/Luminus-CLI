# Luminus CLI

Luminus is a terminal-native AI assistant written in Rust. It provides a responsive Ratatui TUI, streaming conversations, provider switching, model selection, model discovery, cancellation, context accounting, and isolated child agents.

> Current status: active development. The offline fake provider is available by default; real providers are opt-in through environment variables.

## Features

- Responsive terminal UI built with Ratatui and Crossterm.
- Offline deterministic `FakeProvider` fallback; no API key is required to start.
- OpenAI-compatible provider using `reqwest` with Rustls TLS.
- Streaming SSE responses plus regular JSON completion fallback.
- Runtime provider selection with `/provider`.
- Role-based model selector (`Ctrl+M`) and `/model <role>`.
- Provider model discovery through `/discover` and `GET /models`.
- Persistent JSON sessions with `/save`, `/sessions`, and `/load`.
- Atomic session writes with sanitized names and platform-aware data directories.
- Request cancellation with `Esc` and `Ctrl+C`.
- Child agents using `/spawn <prompt>` with separate lifecycle, output, and cancellation state.
- Single-active-child-agent policy to prevent request-state conflicts.
- Context-window token accounting with deterministic whitespace estimation.
- Terminal cleanup on normal exit and terminal guard teardown.

## Requirements

For building from source on Windows, macOS, or Linux:

- Rust stable toolchain (Rust 1.85+ recommended; edition 2024)
- Cargo
- A terminal with Unicode support

Install Rust with rustup: https://rustup.rs/

## Installation

### Windows

Using PowerShell or Windows Terminal:

```powershell
winget install Rustlang.Rustup
# Restart the terminal, then:
git clone https://github.com/VanZPro/Luminus-CLI.git
cd Luminus-CLI
cargo build --release
```

The executable is created at `target\\release\\luminus.exe`. Optionally copy it to a directory on `PATH`:

```powershell
New-Item -ItemType Directory -Force "$HOME\\.local\\bin"
Copy-Item target\\release\\luminus.exe "$HOME\\.local\\bin\\"
```

### macOS

```bash
xcode-select --install   # only if build tools are not installed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
git clone https://github.com/VanZPro/Luminus-CLI.git
cd Luminus-CLI
cargo build --release
sudo install -m 755 target/release/luminus /usr/local/bin/luminus
```

On Apple Silicon, Cargo builds a native ARM64 binary by default. Use a cross toolchain if you need another target.

### Linux

Debian/Ubuntu prerequisites:

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev git curl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
git clone https://github.com/VanZPro/Luminus-CLI.git
cd Luminus-CLI
cargo build --release
sudo install -m 755 target/release/luminus /usr/local/bin/luminus
```

Fedora/RHEL prerequisites:

```bash
sudo dnf install gcc gcc-c++ make openssl-devel pkg-config git curl
```

Then run the same Rust installation and build commands above.

## Running

Start offline mode:

```bash
cargo run
# or after release build:
luminus
```

Show CLI metadata:

```bash
luminus --help
luminus --version
```

Offline mode uses the deterministic fake provider and does not contact a network service.

## OpenAI-compatible provider

Configure an OpenAI or compatible endpoint before launching:

### Bash / zsh

```bash
export OPENAI_API_KEY='your-key'
export OPENAI_BASE_URL='https://api.openai.com/v1' # optional
export OPENAI_MODEL='gpt-4o-mini'                  # optional
luminus
```

### PowerShell

```powershell
$env:OPENAI_API_KEY = 'your-key'
$env:OPENAI_BASE_URL = 'https://api.openai.com/v1'
$env:OPENAI_MODEL = 'gpt-4o-mini'
luminus
```

`LUMINUS_OPENAI_API_KEY`, `LUMINUS_OPENAI_BASE_URL`, and `LUMINUS_OPENAI_MODEL` are accepted as namespaced alternatives. Never commit credentials or paste them into bug reports.

## Commands and shortcuts

| Input | Action |
|---|---|
| `/help` | Show command help |
| `/about` | Show application information |
| `/clear` | Clear conversation and agent state |
| `/exit` | Exit the TUI |
| `/model <role>` | Select a model role |
| `/models` | List configured role/model mappings |
| `/discover` | Query the active provider's `/models` endpoint |
| `/save <name>` | Save the current conversation to disk |
| `/sessions` | List saved conversations |
| `/load <name>` | Restore a saved conversation |
| `/provider` | Show the current provider |
| `/provider fake` | Switch to offline fake provider |
| `/provider openai` | Switch to configured OpenAI-compatible provider |
| `/spawn <prompt>` | Start one isolated child-agent request |
| `Ctrl+M` | Open model selector |
| `Esc` | Cancel active child agent or main request |
| `Ctrl+C` | Cancel the main request; press again when idle to exit |

## Debugging results and known behavior

The current debugging pass verified:

- Release build succeeds on the development Windows environment.
- Workspace tests, formatting, and Clippy pass with warnings denied.
- The default provider remains fake when no API key is configured.
- Invalid configured endpoints are surfaced as configuration errors rather than silently falling back.
- Streaming SSE and non-streaming JSON completion responses are both supported.
- Child-agent events are routed separately from the main conversation by request ID.
- A second `/spawn` request is rejected while another child agent is running.
- `Esc` cancels the child agent before cancelling the main request.
- Model discovery is asynchronous, so the TUI event loop is not blocked.
- `/discover` displays provider errors in the conversation instead of crashing.
- API keys are redacted from provider debug output.

Known limitations:

- Sessions are persisted as JSON under the platform data directory. Set `LUMINUS_DATA_DIR` to override it; `/clear` still removes only the current in-memory conversation.
- The model catalog is currently in-memory and role-based.
- Only one child agent may run at a time.
- Tool execution, permissions/approvals, MCP, plugins, LSP, and packaged installers are roadmap items.
- `/discover` requires the active provider to implement the OpenAI-compatible `/models` endpoint.

When reporting a bug, include the operating system, terminal emulator, Luminus commit, command being used, and sanitized error output. Do not include API keys.

## Development

Run the quality gates from the repository root:

```bash
cargo fmt -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release
```

Build artifacts in `target/` and code intelligence cache files are ignored by Git.

## Roadmap

- Persistent sessions and conversation resume.
- Safe coding tools with explicit permissions and approval UI.
- Multi-agent orchestration beyond the single-child policy.
- MCP/plugins/skills integration.
- LSP and code intelligence.
- Cross-platform packaged releases and automated GitHub Actions builds.

## License

MIT

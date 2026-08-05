# Luminus tasks

## Milestone 1

- [x] Typed events and application reducer
- [x] Slash commands: help, about, clear, exit
- [x] Deterministic cancellable fake provider
- [x] Responsive black-and-blue TUI
- [x] Safe terminal lifecycle
- [x] Unit, integration, and render tests
- [x] Formatting, Clippy, tests, and release build

## Verification notes

Milestone 1 library and binary checks pass on Windows with stable Rust. The interactive TUI has been compiled and the `--help` path has been smoke-tested. A full interactive PTY session and panic restoration still require manual terminal validation.

## Deferred

## Phase 2 completed

- [x] Provider capability and model metadata contract
- [x] Deterministic multi-delta streaming and cancellation
- [x] Typed context budget and deterministic token estimation
- [x] Status/composer TUI region with model/mode/context/safety indicators
- [x] Phase 2 tests, formatting, Clippy, and release build

## Phase 6 completed

- [x] Connect real composer text to the rendered composer region
- [x] Keyboard double-typing fix (KeyEventKind::Press filter)
- [x] Responsive viewport fix for tiny terminals
- [x] Structured tool activity events and cards

## Phase 7 completed

- [x] Interactive model selector overlay (Ctrl+M, arrow keys, Enter, Esc)
- [x] Model selection state and centered responsive popup rendering

## Phase 8 completed

- [x] Real OpenAI-compatible HTTP provider adapter
- [x] Streaming SSE and non-streaming JSON completion handling
- [x] Runtime provider selection with fake fallback
- [x] Environment credentials: OPENAI_API_KEY / OPENAI_BASE_URL / OPENAI_MODEL
- [x] `/provider openai`, `/provider fake`, and current-provider status
- [x] Stub-transport tests for streaming, cancellation, API errors, and env config
- [x] Formatting, Clippy, workspace tests, and release build

## Phase 9 completed

- [x] `/spawn <prompt>` command with validation and help text
- [x] Independent child-agent lifecycle and event routing
- [x] Single active-agent policy with cancellation priority
- [x] Agent status/output rendering in the conversation view
- [x] Reducer, command, and TUI integration tests
- [x] Formatting, Clippy, workspace tests, and release build

## Phase 10 completed

- [x] Model discovery via `GET /models` for OpenAI-compatible providers
- [x] `/discover` async command with provider-event routing
- [x] Fake provider remains offline by default
- [x] Formatting, Clippy, workspace tests, and release build

## Phase 11 completed

- [x] Atomic JSON session persistence (`/save`, `/sessions`, `/load`)
- [x] Sanitized session names and platform-aware data directory
- [x] `LUMINUS_DATA_DIR` override support
- [x] Formatting, Clippy, workspace tests, and release build

## Phase 12 completed

- [x] Permission-gated tool registry (`read_file`, `write_file`, `list_dir`, `run_shell`, `http_get`)
- [x] `/tools` listing and `/tool <name> <args...>` invocation
- [x] Approval overlay (`UiMode::Approval`) with Y/Enter and N/Esc
- [x] Network tool explicitly disabled in this phase
- [x] Formatting, Clippy, workspace tests, and release build

## Next priorities

- [ ] Real providers and credentials
- [ ] Skill / MCP / plugin support
- [ ] LSP and production installers
- [ ] Advanced agent orchestration

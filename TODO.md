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

## Next priorities

- [ ] Provider model catalog/discovery endpoint
- [ ] Sessions and context persistence
- [ ] Coding tools, permissions, approvals, and diffs

- Real providers and credentials
- Coding tools, permissions, approvals, and diffs
- Sessions and context persistence
- Skills, MCP, plugins, agents, and missions
- LSP and production installers

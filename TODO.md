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

## Next priorities

- [ ] Add interactive model selector overlay (currently command-only)
- [ ] Add real OpenAI-compatible provider adapter
- [ ] Wire real provider credentials from environment

- Real providers and credentials
- Coding tools, permissions, approvals, and diffs
- Sessions and context persistence
- Skills, MCP, plugins, agents, and missions
- LSP and production installers

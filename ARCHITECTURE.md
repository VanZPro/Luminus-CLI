# Luminus architecture

This document describes the architecture currently implemented in this repository through Phase 12. It intentionally does not claim the complete architecture requested by `INTRUKSI.md`; unavailable and experimental areas are called out explicitly.

## Current shape

Luminus is currently a single Rust package with explicit module boundaries rather than a multi-crate workspace. The boundaries are intended to keep TUI, state, providers, sessions, agents, and tools independently testable as the project grows.

```text
terminal input ----+
provider stream ---+--> AppEvent --> App reducer --> state --> renderer
session events ----+                     |
tool approvals ----+                     +--> runtime/provider/tool effects
resize/timer -------+
```

The application is centered on typed events and a reducer-style `App::update` flow. Rendering reads application state and should not own provider, agent, session, or tool logic. Provider requests use stable request IDs and cancellation tokens. Terminal setup/teardown is guarded so normal exits and many error paths restore the terminal.

## Implemented modules and responsibilities

- `main.rs` — CLI entry point and TUI runtime wiring.
- `app.rs` — central state, reducer logic, command dispatch, provider/tool/session/agent state transitions.
- `event.rs` — typed application/provider events.
- `command.rs` — slash-command parsing for implemented commands.
- `provider` / `providers::*` — fake provider, provider contracts, and OpenAI-compatible runtime adapter.
- `model.rs` — model roles/catalog selection foundations.
- `context.rs` — deterministic context/token accounting foundations.
- `tool_activity.rs` — tool activity events and presentation state.
- `tools.rs` — Phase 12 typed tool registry and approval-gated tool execution.
- `session.rs` — atomic JSON session save/list/load using sanitized names and a platform-aware data directory.
- `agent.rs` — limited child-agent request state and single-active-child policy.
- `tui::*` — Ratatui rendering, theme, logo, overlays, status, model selector, approval UI, and responsive presentation.

## Provider architecture

Implemented provider behavior is foundation-level:

- The fake provider is deterministic, offline, streaming, and cancellable.
- The OpenAI-compatible provider can use environment configuration, stream SSE responses, fall back to non-streaming JSON completions, and query `/models` for discovery when the endpoint supports it.
- Runtime provider selection exists between fake and OpenAI-compatible modes.

Unavailable provider requirements from `INTRUKSI.md` include complete provider-specific adapters for Anthropic, Gemini, OpenRouter, Ollama, native OS keychain credential storage, full capability negotiation, usage accounting, retry classification across all providers, vision, structured output, and cancellation semantics beyond the implemented paths.

## Session architecture

The current session system persists conversations as JSON files. Writes are atomic, names are sanitized, and the storage directory can be overridden with `LUMINUS_DATA_DIR`.

Unavailable session requirements include durable JSONL/event-store output, session resume tree, fork, compaction, export, searchable history, and mission persistence. Current `/load` restores a saved conversation file; it is not the complete resume/fork model described in `INTRUKSI.md`.

## Tool and approval architecture

Phase 12 adds a typed registry and approval flow:

1. `/tools` lists registered tools.
2. `/tool <name> <args...>` creates a pending tool request.
3. The approval overlay requires explicit user acceptance or rejection.
4. Approved tools run through the registry.

Registered current tools are `read_file`, `write_file`, `list_dir`, `run_shell`, and `http_get`. `http_get` is intentionally disabled/unavailable in Phase 12.

### Security boundaries and gaps

The current approval flow is not a complete security model. The following `INTRUKSI.md` requirements remain unavailable or unproven:

- Full `allow`/`ask`/`deny` policy engine.
- Sandboxed shell execution.
- Permission wrapping for MCP and plugin tools.
- Directory traversal and symlink escape defenses.
- Default denial of `.env` and credential files.
- Hash-aware/stale-edit rejection.
- Diff inspection before accepting edits.
- Secret redaction across all logs, transcripts, crash reports, and doctor output.
- Cancellation for every tool and background process management.
- Intelligent truncation with full output retained.
- Security tests for traversal, symlinks, credential reads, shell matching, denied plugin/MCP tools, redaction, and stale edits.

Until these are implemented and tested, the tool system should be considered approval-gated but not sandboxed or production-hardened.

## Agent architecture

The implemented agent capability is intentionally limited. `/spawn <prompt>` starts one child-agent provider request with separate lifecycle, output, request ID, and cancellation state. The application enforces one active child agent at a time.

Unavailable/experimental agent requirements include configurable primary agents, custom Markdown/YAML agent definitions, structured schema-validated subagent results, parallel orchestration, isolated Git worktrees, persistent missions, task panels, and multi-agent scheduling.

## UI architecture

The UI uses Ratatui and Crossterm. Current implemented pieces include:

- Black background and blue LUMINUS branding.
- Responsive TUI foundations.
- Conversation rendering and status/composer areas.
- Model selector overlay.
- Tool cards/activity rendering.
- Approval overlay.
- Cancellation handling for active requests.

Unavailable UI requirements include the full onboarding flow, complete command palette, configurable keybindings, file/agent/skill autocomplete, external editor integration, complete changed-files/diff views, session tree, mission panel, and all narrow/wide layout details requested by `INTRUKSI.md`.

## Extensibility and code intelligence

Skills, custom commands, hooks, plugin protocol, MCP stdio, MCP Streamable HTTP, and LSP/code-intelligence features are not implemented. Any documentation or UI should mark them unavailable until real behavior exists.

## Noninteractive and automation support

The current architecture uses typed events that could later support JSONL/noninteractive output, but noninteractive CLI automation is not implemented. Do not document it as available.

## Production readiness

This repository is active development/foundation-level through Phase 12. It should not be described as production-ready. Production readiness still requires the acceptance criteria in `INTRUKSI.md`, including full security hardening, cross-platform CI, TUI snapshots, doctor diagnostics, provider/keychain completion, packaging, documentation, and complete format/lint/test/build verification across supported platforms.

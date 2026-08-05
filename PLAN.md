# Luminus implementation plan

This roadmap records the implementation that is actually present in this repository. A phase is marked **completed** only for the delivered foundation described below; completion does not mean that every product requirement in `INTRUKSI.md` has been implemented. Features not present are explicitly marked **unavailable** or **experimental** rather than presented as functional.

## Completed implementation phases

### Phase 1 — Foundation (completed foundation scope)

- Rust application/package with a typed event model and application reducer.
- Ratatui/Crossterm terminal UI with the black-and-blue LUMINUS presentation.
- Slash-command parsing for the implemented commands.
- Deterministic, cancellable offline fake provider.
- Terminal setup/cleanup guard and responsive rendering foundations.

**Not complete from the full Phase 1 requirements:** the full CLI/onboarding experience, complete command palette, and all requested responsive interaction behavior are not implemented.

### Phase 2 — Agent conversation (completed foundation scope)

- Provider abstraction and capability/model metadata contracts.
- Deterministic streaming deltas and cancellation.
- Model/context accounting using deterministic whitespace estimation.
- Model/mode/context/safety status and tool-activity UI foundations.

**Not complete:** the full provider capability negotiation, usage/token accounting, all built-in agents/tools, and the complete composer/keybinding specification remain unavailable.

### Phase 3 — Safe coding tools (completed limited scope)

- Typed permission-gated tool registry.
- Implemented tool requests for `read_file`, `write_file`, `list_dir`, and `run_shell`.
- Explicit approval overlay and approval/rejection flow.
- `http_get` is registered as a visibly disabled/unavailable network tool.

**Security gap:** this is not the full `allow`/`ask`/`deny` permission engine specified by `INTRUKSI.md`. There is no demonstrated sandbox, path canonicalization/traversal protection, symlink escape protection, credential-file deny policy, hash-aware edit rejection, diff review, or complete Git tool surface. Approved shell execution is not a sandbox.

### Phase 4 — Persistence (completed limited scope)

- Atomic JSON conversation persistence.
- `/save`, `/sessions`, and `/load` commands.
- Sanitized session names, platform-aware data directory, and `LUMINUS_DATA_DIR` override.

**Unavailable:** resume/fork/tree/compaction/export as specified are not implemented. Persistence is conversation JSON, not a complete event store or mission store.

### Phase 5 — Code intelligence (foundation only)

- No Phase 5 feature set is implemented in the current source.

**Unavailable:** LSP client, diagnostics, symbols, references, rename, and changed-files panel.

### Phase 6 — Extensibility (foundation only)

- No skills, custom commands, hooks, plugin protocol, or MCP implementation is present.

**Unavailable:** skills, MCP stdio/Streamable HTTP, and plugins. They must not be treated as supported configuration or commands.

### Phase 7 — Agents and missions (limited scope)

- `/spawn <prompt>` starts one isolated child-agent request.
- Child-agent lifecycle/events/output/cancellation are routed separately from the main request.
- A single-active-child-agent policy prevents request-state conflicts.

**Experimental/limited:** this is not the specialized-agent/subagent system in `INTRUKSI.md`. Structured schema-validated results, configurable agents, worktree isolation, orchestration, persistent missions, and a plan/progress panel are unavailable. Only one child agent may run at a time.

### Phase 8 — Production polish (selected provider/UI scope)

- Real OpenAI-compatible HTTP provider using streaming SSE and non-streaming JSON fallback.
- Runtime fake/OpenAI provider selection and environment-based configuration.
- Interactive model selector.

**Unavailable:** `doctor`, installers, shell completions, OS keychain integration, cross-platform CI validation, profiling, and production security hardening.

### Phase 9 — Child-agent request flow (completed limited scope)

- Validated `/spawn` command, separate request IDs/state, cancellation priority, and TUI status/output rendering.

This phase extends the limited Phase 7 capability; it does not add general orchestration or persistent missions.

### Phase 10 — Model discovery (completed limited scope)

- Asynchronous `/discover` model listing for OpenAI-compatible providers implementing `GET /models`.
- Provider-event routing and surfaced provider errors.
- Fake provider remains offline by default.

**Unavailable:** discovery for providers without that endpoint and the complete model catalog/role-management system.

### Phase 11 — Session persistence (completed)

- Atomic JSON session save/list/load behavior, sanitized names, platform data directories, and environment override.

This does not imply session fork/tree/compaction/export support.

### Phase 12 — Approval-gated coding tools (completed limited scope)

- Permission-gated registry and `/tools` listing.
- `/tool <name> <args...>` request flow.
- Approval UI with accept/reject keyboard flow.
- Network `http_get` remains explicitly disabled.

**Security gaps:** approval is not a complete policy engine or sandbox. Tools execute synchronously after approval; long-running/background tool streams, cancellation for all tools, intelligent output truncation, full audit/event durability, diff inspection, secret redaction across all logs, and the security tests listed by `INTRUKSI.md` are not established by this phase.

## Remaining roadmap

1. Implement the complete permission model (`allow`/`ask`/`deny`) around every built-in, MCP, and plugin tool; add safe path handling, credential-file protections, symlink/traversal defenses, stale-edit rejection, diffs, and sandboxing.
2. Add the remaining Git, filesystem, runtime, code-intelligence, and interaction tools with cancellation and structured progress.
3. Implement durable event-backed sessions with resume, fork, tree, compaction, and export.
4. Add LSP/code intelligence.
5. Add skills, custom commands, hooks, plugins, and MCP (stdio and Streamable HTTP).
6. Expand agents into configurable specialized subagents and missions with structured results and worktree isolation.
7. Add `luminus doctor`, keychain support, installers, completions, cross-platform CI, profiling, documentation, and security hardening.
8. Add noninteractive/JSONL automation only when implemented; it is currently unavailable.

## Verification expectation

Each future phase must leave the application buildable and testable. Before calling the project production-ready, run the formatting, Clippy, test, build, security, TUI, and cross-platform checks required by `INTRUKSI.md`. The current repository should be described as active development, not production-ready.

# Luminus implementation plan

Luminus will be delivered as vertical, testable milestones. The first milestone is an offline terminal conversation loop: responsive branding, prompt input, deterministic streaming, cancellation, and reliable terminal cleanup.

Later milestones add real providers, safe coding tools and permissions, persistence, code intelligence, extensibility, agents, missions, and production hardening. Unimplemented capabilities will not appear functional.

## Milestone 1

1. Establish typed application and provider events.
2. Implement slash-command parsing and deterministic state transitions.
3. Implement a cancellable offline fake provider.
4. Build a black-and-blue responsive Ratatui interface.
5. Restore the terminal after exit, error, cancellation, or panic.
6. Verify behavior with unit, integration, render, lint, and build checks.

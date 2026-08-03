# Luminus architecture

Milestone 1 uses one package with explicit module boundaries. This avoids empty crates while preserving extraction points.

Input and provider tasks emit typed `AppEvent` values. `App::update` is the state reducer and returns effects for the runtime. Rendering reads state and never owns agent logic. Providers do not depend on terminal code. Terminal setup is guarded by RAII and cleanup is idempotent.

```text
terminal input ----+
provider stream ---+--> AppEvent --> App reducer --> state --> renderer
timer/resize ------+                     |
                                         +--> runtime effects
```

Core events contain no Ratatui or ANSI types so they can later drive JSONL output and durable session events. Each provider request has a stable ID and exactly one terminal outcome: completed, cancelled, or failed.

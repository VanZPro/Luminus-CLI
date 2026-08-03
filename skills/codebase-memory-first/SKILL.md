---
name: codebase-memory-first
description: Use before exploring any codebase. Query codebase-memory-mcp (or code graph tools) first so you do not read the whole project. Triggers on coding, refactor, debug, architecture, impact analysis, find callers, navigate repo.
---

# Codebase Memory First (retrieve before read)

## Goal
Never dump or walk the entire repository. Prefer structural/graph retrieval, then load a skill, then edit only relevant files.

## Mandatory order
1. **Index check** — if the project is not indexed in codebase-memory-mcp, index it once.
2. **Retrieve** — use MCP tools from `codebase-memory-mcp` (search, trace, architecture, impact, routes, call chains). Prefer graph queries over `find`/`grep` over full-file reads.
3. **Skill** — after you know stack/context, load the matching skill (docker, typescript, test, etc.).
4. **Act** — edit/run only files pointed to by retrieval.
5. **Verify** — re-query impact/callers if the change is structural.

## Hard rules
- Do **not** start with recursive directory listing of the whole repo.
- Do **not** open dozens of files "to understand the project".
- If a tool result is enough, do not re-read the same file raw.
- If codebase-memory-mcp is unavailable, say so and fall back to targeted `rg`/`grep` with narrow paths — still no full-tree soak.

## When this skill applies
Any task in an existing git/project tree: feature work, bugfix, review, refactor, onboarding to a repo.

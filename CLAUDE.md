# CLAUDE.md — Project Context for Claude Code

## What is this?

Clawed is a minimal self-sufficient Rust chat agent. It's a simplified extraction from the ironclaw project, keeping the core agent loop with LLM provider, tool calls, and skill system.

## Build & Run

```bash
cargo build                                    # Build
cargo run                                      # Interactive REPL
cargo run -- -p "prompt"                       # Single-shot mode
RUST_LOG=clawed=debug cargo run                # Debug logging
```

## Project Structure

- `src/llm/` — LLM abstraction layer using rig-core. `provider.rs` defines the trait, `rig_adapter.rs` wraps rig-core, `reasoning.rs` handles the respond-with-tools loop and response cleaning.
- `src/tools/` — Tool trait + registry in `mod.rs`, implementations in `shell.rs` and `file.rs`. Tools take `serde_json::Value` params and `ToolContext`.
- `src/skills/` — SKILL.md parsing, keyword/regex scoring, trust-based tool attenuation. Skills are loaded from `~/.clawed/skills/<name>/SKILL.md`.
- `src/agent.rs` — The core multi-turn agent loop: build context, call LLM, execute tools, repeat.
- `src/safety/` — Tool output sanitization and injection pattern detection.

## Key Patterns

- Tools implement the `Tool` trait with `name()`, `description()`, `parameters_schema()`, `execute()`.
- The agent loop in `run_loop()` alternates between LLM calls and tool execution until the LLM returns text (no tool calls).
- `force_text = true` on the last turn guarantees termination.
- Shell tool inherits full env, only strips secrets matching `API_KEY`, `SECRET`, `TOKEN`, `PASSWORD`, `CREDENTIAL`, `ANTHROPIC_*`, `OPENAI_*`, `AWS_SECRET`.
- Tool call recovery: if the LLM emits `<tool_call>` XML instead of structured tool use, the reasoning engine recovers it.

## Known Gaps (see docs/missing-features.md)

- Only Anthropic backend; rig-core supports OpenAI/Ollama too.
- No streaming, no token usage reporting, no conversation persistence.
- No tool confirmation/approval for destructive operations.
- No workspace skills (project-local `.clawed/skills/`).

## Dependencies

- `rig-core` 0.30 — LLM abstraction (Anthropic, OpenAI, Ollama)
- `tokio` — async runtime
- `clap` — CLI parsing
- `rustyline` — REPL line editing
- `serde` / `serde_json` / `serde_yml` — serialization
- `regex` — skill pattern matching
- `sha2` — skill content hashing

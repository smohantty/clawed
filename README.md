# clawed

Minimal self-sufficient Rust chat agent with tool use and skills.

Built on [rig-core](https://github.com/0xPlaygrounds/rig) for API-backed providers (Anthropic, OpenAI, Gemini) with an additional `claude -p` backend for keyless single-shot use. Designed to run locally on any machine — no containers, no sandboxes required.

## Quick Start

```bash
# Anthropic (default)
echo 'ANTHROPIC_API_KEY=sk-ant-...' > .env
cargo run

# OpenAI
echo -e 'CLAWED_BACKEND=openai\nOPENAI_API_KEY=sk-...' > .env
cargo run

# Gemini
echo -e 'CLAWED_BACKEND=gemini\nGEMINI_API_KEY=...' > .env
cargo run

# Claude CLI (single-shot only; no API key env var required)
echo 'CLAWED_BACKEND=claude_cli' > .env
cargo run -- -p "explain this codebase"

# Single-shot mode
cargo run -- -p "what files are in the current directory?"

# Override model for any backend
cargo run -- --model gpt-4o-mini -p "explain this codebase"
```

## CLI Options

```
clawed [OPTIONS]

Options:
  -p, --prompt <PROMPT>        Single-shot mode: execute and exit
      --model <MODEL>          LLM model (overrides active backend's default)
      --no-skills              Disable skill loading
      --max-turns <MAX_TURNS>  Max agent iterations [default: 50]
  -h, --help                   Print help
```

## Architecture

```
src/
  main.rs              CLI entry point (clap)
  config.rs            .env loading, ClawedConfig
  agent.rs             Multi-turn agent loop
  repl.rs              Interactive REPL (rustyline)
  error.rs             Error types
  llm/
    mod.rs             Provider factory
    provider.rs        LlmProvider trait, ChatMessage, ToolCall, ToolDefinition
    reasoning.rs       Reasoning engine, respond_with_tools, tool call recovery
    rig_adapter.rs     rig-core CompletionModel adapter
    claude_cli_provider.rs  `claude -p` adapter for single-shot calls
  tools/
    mod.rs             Tool trait, ToolRegistry
    shell.rs           Shell command execution
    file.rs            read_file, write_file, list_dir, apply_patch
    builtin.rs         echo, time, json utility tools
  skills/
    mod.rs             Types: LoadedSkill, SkillManifest, SkillTrust
    parser.rs          SKILL.md frontmatter + body parsing
    registry.rs        Discovery from ~/.clawed/skills/
    selector.rs        Deterministic keyword/tag/regex scoring
    attenuation.rs     Trust-based tool filtering
  safety/
    mod.rs             Output sanitization, injection detection
```

## Tools

| Tool | Description |
|------|-------------|
| `shell` | Execute shell commands (blocked dangerous patterns, sensitive env stripped) |
| `read_file` | Read file content with optional line range (max 1 MiB) |
| `write_file` | Write/create files with auto-mkdir (max 5 MiB) |
| `list_dir` | List directory contents with types and sizes |
| `apply_patch` | Search-and-replace file editing (requires unique match) |
| `echo` | Echo back input message (useful for testing tool execution) |
| `time` | Get current time, parse timestamps, or calculate time differences |
| `json` | Parse, query, stringify, and validate JSON data |

The shell tool inherits the full user environment and only strips sensitive variables (`API_KEY`, `SECRET`, `TOKEN`, etc.), so tools like `curl`, `git`, `docker` work normally.

## Skills

Skills are prompt extensions loaded from `~/.clawed/skills/<name>/SKILL.md`.

### SKILL.md Format

```yaml
---
name: my-skill
version: "1.0.0"
description: What this skill does
activation:
  keywords:
    - deploy
    - kubernetes
  patterns:
    - "k8s|kubectl"
  tags:
    - devops
  max_context_tokens: 2000
---

# My Skill

Your prompt instructions here. These get injected into the system
prompt when the skill activates based on keyword/pattern matching.
```

### Activation Scoring

Skills are scored against user input and the top 3 (within a 4000 token budget) are activated:

- **Keyword match** (whole word): +10 pts (cap 30)
- **Keyword match** (substring): +5 pts (cap 30)
- **Tag match**: +3 pts (cap 15)
- **Regex pattern match**: +20 pts (cap 40)

### Trust Levels

| Trust | Tool Access |
|-------|-------------|
| `Trusted` | Full access to all tools |
| `Installed` | Read-only tools only (`time`, `echo`, `json`, `read_file`, `list_dir`) |

User skills (from `~/.clawed/skills/`) default to `Trusted`. If any active skill has `Installed` trust, tool access is restricted to the read-only set.

## REPL Mode

Start the REPL with `cargo run` (no `-p` flag). Conversation context persists across messages — the agent remembers what you discussed and what tools returned. Use `/clear` to reset.

```
$ cargo run
clawed v0.1.0
Type /help for commands, /quit to exit.

clawed> what files are in src/?
[tool: list_dir]

Here are the files in src/: main.rs, config.rs, agent.rs, repl.rs, ...

clawed> now read main.rs and explain the startup sequence
[tool: read_file]

The startup sequence in main.rs works as follows: ...

clawed> /clear
(conversation cleared)

clawed> /quit
Goodbye!
```

### Commands

| Command | Description |
|---------|-------------|
| `/quit`, `/exit`, `/q` | Exit |
| `/clear` | Reset conversation context |
| `/tools` | List available tools |
| `/help` | Show help |

## Backends

Clawed supports four LLM backends via the `CLAWED_BACKEND` env var:

| Backend | Value | API Key Env Var | Default Model |
|---------|-------|-----------------|---------------|
| Anthropic | `anthropic` (default) | `ANTHROPIC_API_KEY` | `claude-sonnet-4-20250514` |
| OpenAI | `openai` | `OPENAI_API_KEY` | `gpt-4o` |
| Gemini | `gemini` | `GEMINI_API_KEY` | `gemini-2.5-flash` |
| Claude CLI | `claude_cli` | *(none)* | `opus4.6` |

Only the API key for the selected API backend is required. The `--model` CLI flag overrides the active backend's default model.

`claude_cli` backend notes:
- Uses `claude -p --input-format text --output-format json --tools ""` under the hood.
- `CLAUDE_CLI_MODEL=opus4.6` is normalized to `claude-opus-4-6` for CLI compatibility.
- Single-shot mode only (`-p`); REPL is intentionally unsupported for this backend.

## Configuration

| Env Variable | Default | Description |
|---|---|---|
| `CLAWED_BACKEND` | `anthropic` | LLM backend (`anthropic`, `openai`, `gemini`, `claude_cli`) |
| `ANTHROPIC_API_KEY` | *required for anthropic* | Anthropic API key |
| `CLAWED_MODEL` | `claude-sonnet-4-20250514` | Anthropic model |
| `OPENAI_API_KEY` | *required for openai* | OpenAI API key |
| `OPENAI_MODEL` | `gpt-4o` | OpenAI model |
| `GEMINI_API_KEY` | *required for gemini* | Gemini API key |
| `GEMINI_MODEL` | `gemini-2.5-flash` | Gemini model |
| `CLAUDE_CLI_MODEL` | `opus4.6` | Claude CLI model (used when `CLAWED_BACKEND=claude_cli`) |
| `CLAUDE_CLI_TIMEOUT_SECS` | `300` | Timeout for each `claude -p` call |
| `CLAWED_LOG_DIR` | `~/.clawed/logs` | Directory for persistent interaction logs |
| `CLAWED_LOG_FILE` | `llm-interactions.log` | Base log filename (daily rotated) |
| `CLAWED_LOG_FILE_FILTER` | `clawed=trace,rig=trace,warn` | Verbosity filter for file logs |
| `CLAWED_SKILLS_DIR` | `~/.clawed/skills` | Skills directory |
| `CLAWED_MAX_TURNS` | `50` | Max agent loop iterations |

## Logging

Clawed now writes extensive, turn-by-turn logs to file by default. This is designed for debugging agent behavior and includes:
- LLM request payloads and metadata for each turn
- LLM raw responses, cleaned responses, token usage
- Structured tool calls and recovered tool calls
- Tool execution inputs and outputs (including wrapped/sanitized tool content)
- `skill_list` and `load_skill` tool execution details

Default log path pattern:
- `~/.clawed/logs/llm-interactions.log.YYYY-MM-DD`

Console logs still follow `RUST_LOG`. File logs use `CLAWED_LOG_FILE_FILTER` (default `clawed=trace,rig=trace,warn`).

## Safety

- Shell tool blocks dangerous commands (`rm -rf /`, fork bombs, `mkfs`, etc.)
- Shell tool detects injection patterns (base64-to-shell, netcat, DNS exfiltration)
- Tool output truncated at 128 KiB
- Prompt injection pattern detection in tool output
- Thinking/reasoning tags stripped from LLM responses
- Orphaned tool_result messages rewritten as user messages
- Agent loop forced to text-only on final turn to guarantee termination

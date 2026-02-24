# clawed

Minimal self-sufficient Rust chat agent with tool use and skills.

Built on [rig-core](https://github.com/0xPlaygrounds/rig) for LLM abstraction, starting with Anthropic. Designed to run locally on any machine — no containers, no sandboxes required.

## Quick Start

```bash
# Set your API key
echo 'ANTHROPIC_API_KEY=sk-ant-...' > .env

# Interactive REPL
cargo run

# Single-shot mode
cargo run -- -p "what files are in the current directory?"

# Override model
cargo run -- --model claude-sonnet-4-20250514 -p "explain this codebase"
```

## CLI Options

```
clawed [OPTIONS]

Options:
  -p, --prompt <PROMPT>        Single-shot mode: execute and exit
      --model <MODEL>          LLM model [default: claude-sonnet-4-20250514]
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
  tools/
    mod.rs             Tool trait, ToolRegistry
    shell.rs           Shell command execution
    file.rs            read_file, write_file, list_dir, apply_patch
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
| `Installed` | Read-only tools only (`read_file`, `list_dir`) |

User skills (from `~/.clawed/skills/`) default to `Trusted`. If any active skill has `Installed` trust, tool access is restricted to the read-only set.

## REPL Commands

| Command | Description |
|---------|-------------|
| `/quit`, `/exit`, `/q` | Exit |
| `/clear` | Reset conversation context |
| `/tools` | List available tools |
| `/help` | Show help |

## Configuration

| Env Variable | Default | Description |
|---|---|---|
| `ANTHROPIC_API_KEY` | *required* | Anthropic API key |
| `CLAWED_MODEL` | `claude-sonnet-4-20250514` | Model to use |
| `CLAWED_BACKEND` | `anthropic` | LLM backend |
| `CLAWED_SKILLS_DIR` | `~/.clawed/skills` | Skills directory |
| `CLAWED_MAX_TURNS` | `50` | Max agent loop iterations |

## Safety

- Shell tool blocks dangerous commands (`rm -rf /`, fork bombs, `mkfs`, etc.)
- Shell tool detects injection patterns (base64-to-shell, netcat, DNS exfiltration)
- Tool output truncated at 128 KiB
- Prompt injection pattern detection in tool output
- Thinking/reasoning tags stripped from LLM responses
- Orphaned tool_result messages rewritten as user messages
- Agent loop forced to text-only on final turn to guarantee termination

# Missing Features

Status of the skill and tool systems relative to what a full-featured agent would have.

---

## Skill System

### Skill Context Not Injected (Critical)

The skill activation pipeline works end-to-end — skills are discovered, parsed, scored, and selected. But the final step is broken: `build_skill_context()` in `agent.rs` computes the XML skill blocks, then discards the result:

```rust
let skill_context = build_skill_context(&active_skills);
// ...
let _ = skill_context; // computed but unused
```

The `Reasoning` struct has a `skill_context: Option<String>` field that should receive this, but `agent.rs` never calls `reasoning.with_skill_context()` or otherwise injects it. The system prompt in `reasoning.rs` has a skills section placeholder that reads from `self.skill_context`, but it's always `None`.

**Fix:** Pass the skill context to the reasoning engine, either by setting it on the `Reasoning` struct or by including it in `ReasoningContext`.

### No Workspace Skills

Skills only load from `~/.clawed/skills/` (user directory). There's no support for project-local skills (e.g., `.clawed/skills/` in the repo root). Ironclaw supported both user and workspace skill directories.

### No Skill-Provided Tools

Skills can only attenuate (restrict) the existing tool set. They cannot register new tools. A skill that wanted to add a custom tool (e.g., a database query tool) has no mechanism to do so.

### No Skill Gating / Approval

All user skills are auto-trusted. There's no approval flow for first-time skill activation, no content hash verification across runs, and no mechanism to mark a skill as blocked.

### Installed Trust Level Unused

The `SkillTrust::Installed` variant exists and the attenuation logic handles it, but no code path ever creates a skill with `Installed` trust. All discovered skills get `Trusted`. This was intended for skills installed from a hub (like ironclaw's ClawHub integration).

### Content Hash Not Used

Each skill gets a SHA256 content hash computed at load time, but it's never checked or compared. In a full system this would detect skill tampering or enable caching.

---

## Tool System

### No Tool Confirmation / Approval

All tool calls execute immediately without user confirmation. Destructive operations like `shell rm -rf` or `write_file` overwriting important files proceed without a prompt. A production agent should have an approval gate for dangerous operations.

### No Streaming

Tool output and LLM responses are fully buffered. The user sees nothing until the entire response is complete. For long-running shell commands or large LLM outputs, streaming would improve UX significantly.

### No Tool Rate Limiting

The agent can call tools as fast as the loop runs. There's no rate limiting per tool, no cooldown, and no detection of repetitive tool call patterns (beyond the system prompt asking the LLM not to).

### No MCP (Model Context Protocol) Support

No integration with MCP servers. The tool set is fixed at compile time (the 5 dev tools). MCP would allow dynamic tool registration from external services.

### Read-Only Tool Set Too Small

The attenuation system's `READ_ONLY_TOOLS` list is: `time`, `echo`, `json`, `read_file`, `list_dir`. The `time`, `echo`, and `json` tools don't actually exist in clawed — only `read_file` and `list_dir` are real. If an `Installed` skill activates, the agent would have only 2 working tools.

### No Tool Timeout Configuration

All tools share the shell tool's 120-second default timeout. There's no per-tool timeout configuration and no way for the LLM to request a longer timeout for known slow operations.

---

## LLM / Reasoning

### Single Provider Only

Only Anthropic is implemented. The `LlmBackend` enum has one variant. rig-core supports OpenAI, Ollama, and others — adding them requires implementing `create_*_provider()` functions in `llm/mod.rs`.

### No Token Usage Reporting

`TokenUsage` is tracked per call in `RespondOutput` but never surfaced to the user. There's no per-session token counter, no cost estimation, and no budget enforcement.

### No Conversation Persistence

Conversations exist only in memory. Closing the REPL loses all context. There's no save/load, no export, and no session resumption.

### No Context Window Management

No detection of approaching context limits. The message history grows unbounded until the API rejects it. A production agent should summarize or truncate old messages.

### Fixed Temperature / Max Tokens

Both are hardcoded (temperature=0.7, max_tokens=4096). Not configurable via CLI or environment.

---

## REPL

### No Multiline Input

The REPL reads one line at a time. Pasting multi-line code or prompts doesn't work well.

### No `/skills` Command

There's `/tools` to list tools but no command to show loaded skills, their activation criteria, or current trust levels.

### No `/model` Command

No way to switch models mid-session without restarting.

---

## Priority for Fixing

1. **Skill context injection** — the skill system is fully built but the last wire is disconnected
2. **Tool confirmation** — dangerous operations should prompt the user
3. **Token tracking** — at minimum log usage per turn
4. **Streaming** — significant UX improvement
5. **Workspace skills** — project-local skills are a common need
6. **Additional LLM providers** — OpenAI and Ollama via rig-core

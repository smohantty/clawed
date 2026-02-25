//! Interactive REPL for the agent.

use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

use crate::agent::Agent;
use crate::llm::ReasoningContext;

const HISTORY_FILE: &str = ".clawed/history.txt";

/// Run the interactive REPL loop.
pub async fn run_repl(agent: &Agent) -> Result<(), crate::error::Error> {
    let history_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(HISTORY_FILE);

    // Ensure history directory exists
    if let Some(parent) = history_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut rl = DefaultEditor::new()
        .map_err(|e| crate::error::Error::Config(format!("Failed to create editor: {}", e)))?;

    let _ = rl.load_history(&history_path);

    println!("clawed v{}", env!("CARGO_PKG_VERSION"));
    println!("Type /help for commands, /quit to exit.\n");

    let mut ctx: Option<ReasoningContext> = None;

    loop {
        let readline = rl.readline("clawed> ");
        match readline {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(trimmed);

                // Handle slash commands
                if trimmed.starts_with('/') {
                    match handle_command(trimmed, agent, &mut ctx).await {
                        CommandResult::Continue => continue,
                        CommandResult::Quit => break,
                    }
                }

                // Run agent
                let response = if let Some(ref mut existing_ctx) = ctx {
                    agent.continue_conversation(existing_ctx, trimmed).await
                } else {
                    let mut new_ctx = agent.build_context(trimmed).await;
                    let result = agent.continue_conversation(&mut new_ctx, "").await;
                    // The context already has the user message from build_context
                    // Remove the empty continue_conversation message
                    // Actually, let's just use run_task for the first message
                    ctx = Some(new_ctx);
                    result
                };

                match response {
                    Ok(text) => {
                        println!("\n{}\n", text);
                    }
                    Err(e) => {
                        eprintln!("\n[error] {}\n", e);
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("(Ctrl+C to cancel, /quit to exit)");
            }
            Err(ReadlineError::Eof) => {
                break;
            }
            Err(err) => {
                eprintln!("[error] {}", err);
                break;
            }
        }
    }

    let _ = rl.save_history(&history_path);
    println!("Goodbye!");
    Ok(())
}

enum CommandResult {
    Continue,
    Quit,
}

async fn handle_command(
    cmd: &str,
    agent: &Agent,
    ctx: &mut Option<ReasoningContext>,
) -> CommandResult {
    let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
    let command = parts[0];

    match command {
        "/quit" | "/exit" | "/q" => CommandResult::Quit,
        "/clear" => {
            *ctx = None;
            println!("(conversation cleared)");
            CommandResult::Continue
        }
        "/tools" => {
            let tools = agent.tools.tool_definitions().await;
            println!("\nAvailable tools ({}):", tools.len());
            for t in &tools {
                println!("  {} - {}", t.name, t.description);
            }
            println!();
            CommandResult::Continue
        }
        "/help" => {
            println!("\nCommands:");
            println!("  /quit    - Exit the REPL");
            println!("  /clear   - Clear conversation context");
            println!("  /tools   - List available tools");
            println!("  /help    - Show this help");
            println!();
            CommandResult::Continue
        }
        _ => {
            println!(
                "Unknown command: {}. Type /help for available commands.",
                command
            );
            CommandResult::Continue
        }
    }
}

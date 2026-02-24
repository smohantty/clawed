//! clawed — Minimal self-sufficient Rust chat agent.

mod agent;
mod config;
mod error;
mod llm;
mod repl;
mod safety;
mod skills;
mod tools;

use std::sync::Arc;

use clap::Parser;

use crate::agent::Agent;
use crate::config::ClawedConfig;
use crate::llm::Reasoning;
use crate::skills::SkillRegistry;
use crate::tools::ToolRegistry;

#[derive(Parser)]
#[command(name = "clawed", about = "Minimal self-sufficient chat agent")]
struct Cli {
    /// Single-shot autonomous mode: execute the prompt and exit
    #[arg(short, long)]
    prompt: Option<String>,

    /// LLM model to use (overrides the active backend's default)
    #[arg(long)]
    model: Option<String>,

    /// Disable skill loading
    #[arg(long)]
    no_skills: bool,

    /// Maximum agent iterations per task
    #[arg(long, default_value = "50")]
    max_turns: u32,
}

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("clawed=info,warn")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    // Load configuration
    let mut config = match ClawedConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Configuration error: {}", e);
            std::process::exit(1);
        }
    };

    // Apply CLI overrides
    if let Some(model) = cli.model {
        match config.backend {
            config::LlmBackend::Anthropic => config.model = model,
            config::LlmBackend::OpenAi => config.openai_model = model,
            config::LlmBackend::Gemini => config.gemini_model = model,
        }
    }
    config.max_turns = cli.max_turns;
    config.skills_enabled = !cli.no_skills;

    // Create LLM provider
    let llm = match llm::create_llm_provider(&config) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to create LLM provider: {}", e);
            std::process::exit(1);
        }
    };

    // Create tool registry and register dev tools
    let tools = Arc::new(ToolRegistry::new());
    tools.register_dev_tools().await;

    // Load skills
    let mut loaded_skills = Vec::new();
    if config.skills_enabled {
        let mut registry = SkillRegistry::new(config.skills_dir.clone());
        let skill_names = registry.discover_all().await;
        if !skill_names.is_empty() {
            tracing::info!("Loaded {} skills: {:?}", skill_names.len(), skill_names);
        }
        loaded_skills = registry.skills().to_vec();
    }

    // Wrap skills in Arc for shared ownership between agent and tools
    let skills = Arc::new(loaded_skills);

    // Register skill tools (skill_list, load_skill)
    tools.register_skill_tools(skills.clone()).await;

    // Create reasoning engine
    let reasoning = Reasoning::new(llm);

    // Create agent
    let agent = Agent::new(reasoning, tools.clone(), skills, config.max_turns);

    // Run in appropriate mode
    if let Some(prompt) = cli.prompt {
        // Single-shot mode
        match agent.run_task(&prompt).await {
            Ok(response) => {
                println!("{}", response);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        // Interactive REPL mode
        if let Err(e) = repl::run_repl(&agent).await {
            eprintln!("REPL error: {}", e);
            std::process::exit(1);
        }
    }
}

//! clawed — Minimal self-sufficient Rust chat agent.

mod agent;
mod config;
mod error;
mod llm;
mod repl;
mod safety;
mod skills;
mod tools;

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tracing_subscriber::prelude::*;

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

fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    const DEFAULT_CONSOLE_FILTER: &str = "clawed=info,warn";
    const DEFAULT_FILE_FILTER: &str = "clawed=trace,rig=trace,warn";

    let console_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_CONSOLE_FILTER));

    let log_dir = std::env::var("CLAWED_LOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".clawed")
                .join("logs")
        });
    let log_file_name = std::env::var("CLAWED_LOG_FILE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "llm-interactions.log".to_string());
    let file_filter =
        std::env::var("CLAWED_LOG_FILE_FILTER").unwrap_or_else(|_| DEFAULT_FILE_FILTER.to_string());

    let console_layer = tracing_subscriber::fmt::layer().with_target(false);

    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!(
            "[warn] failed to create log directory '{}': {} (file logging disabled)",
            log_dir.display(),
            e
        );
        tracing_subscriber::registry()
            .with(console_layer.with_filter(console_filter))
            .init();
        return None;
    }

    let appender = tracing_appender::rolling::daily(&log_dir, &log_file_name);
    let (non_blocking, guard) = tracing_appender::non_blocking(appender);
    let file_guard = Some(guard);

    let parsed_file_filter = tracing_subscriber::EnvFilter::try_new(file_filter)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_FILE_FILTER));
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_target(true)
        .with_filter(parsed_file_filter);

    tracing_subscriber::registry()
        .with(console_layer.with_filter(console_filter))
        .with(file_layer)
        .init();

    tracing::info!(
        log_dir = %log_dir.display(),
        log_file_name = %log_file_name,
        file_filter = %std::env::var("CLAWED_LOG_FILE_FILTER")
            .unwrap_or_else(|_| DEFAULT_FILE_FILTER.to_string()),
        console_filter = %std::env::var("RUST_LOG")
            .unwrap_or_else(|_| DEFAULT_CONSOLE_FILTER.to_string()),
        "File logging enabled"
    );
    tracing::event!(
        target: "clawed::audit",
        tracing::Level::INFO,
        log_purpose = "turn-by-turn llm/tool interaction trace",
        "Audit logging initialized"
    );

    file_guard
}

#[tokio::main]
async fn main() {
    // Initialize logging (console + persistent file trace)
    let _file_log_guard = init_logging();

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
            config::LlmBackend::ClaudeCli => config.claude_cli_model = model,
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
        if config.backend == config::LlmBackend::ClaudeCli {
            eprintln!("The claude_cli backend supports single-shot mode only. Use -p/--prompt.");
            std::process::exit(1);
        }

        // Interactive REPL mode
        if let Err(e) = repl::run_repl(&agent).await {
            eprintln!("REPL error: {}", e);
            std::process::exit(1);
        }
    }
}

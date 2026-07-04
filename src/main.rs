use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use sovereign_agent::{ChatCompletionRequest, ChatMessage, OpenAiCompatibleClient};
use sovereign_core::{load_config, redact_secret, write_default_config};
use sovereign_fs::{FileSnapshotPolicy, snapshot_tree};
use sovereign_git::snapshot as git_snapshot;
use sovereign_plugin::validate_manifest;
use sovereign_terminal::{BlockTimeline, OutputStream};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "sovereign-term")]
#[command(about = "Local-first agentic terminal runtime")]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Doctor,
    InitConfig {
        #[arg(long)]
        force: bool,
    },
    Providers,
    Chat {
        #[arg(short, long)]
        prompt: String,

        #[arg(short = 'P', long)]
        provider: Option<String>,

        #[arg(long)]
        system: Option<String>,
    },
    Plugin {
        #[command(subcommand)]
        command: PluginCommands,
    },
    Blocks {
        #[command(subcommand)]
        command: BlockCommands,
    },
    Git {
        #[command(subcommand)]
        command: GitCommands,
    },
    Fs {
        #[command(subcommand)]
        command: FsCommands,
    },
}

#[derive(Debug, Subcommand)]
enum PluginCommands {
    Validate { manifest: PathBuf },
}

#[derive(Debug, Subcommand)]
enum BlockCommands {
    Demo,
}

#[derive(Debug, Subcommand)]
enum GitCommands {
    Snapshot {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum FsCommands {
    Snapshot {
        #[arg(default_value = ".")]
        path: PathBuf,

        #[arg(long, default_value_t = 4)]
        max_depth: usize,

        #[arg(long, default_value_t = 2_000)]
        max_entries: usize,

        #[arg(long)]
        include_hidden: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .compact()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Doctor => doctor(cli.config),
        Commands::InitConfig { force } => init_config(cli.config, force),
        Commands::Providers => providers(cli.config),
        Commands::Chat {
            prompt,
            provider,
            system,
        } => chat(cli.config, provider, prompt, system).await,
        Commands::Plugin { command } => match command {
            PluginCommands::Validate { manifest } => validate_plugin(manifest),
        },
        Commands::Blocks { command } => match command {
            BlockCommands::Demo => blocks_demo(),
        },
        Commands::Git { command } => match command {
            GitCommands::Snapshot { path } => git_snapshot_command(path),
        },
        Commands::Fs { command } => match command {
            FsCommands::Snapshot {
                path,
                max_depth,
                max_entries,
                include_hidden,
            } => fs_snapshot_command(path, max_depth, max_entries, include_hidden),
        },
    }
}

fn doctor(config_path: Option<PathBuf>) -> Result<()> {
    let config = load_config(config_path.clone())?;
    println!("Sovereign Term doctor");
    println!(
        "config: {}",
        config_path
            .unwrap_or_else(sovereign_core::default_config_path)
            .display()
    );
    println!(
        "telemetry: {}",
        bool_label(config.privacy.telemetry_enabled)
    );
    println!(
        "cloud handoff: {}",
        bool_label(config.privacy.cloud_handoff_enabled)
    );
    println!("plugins: {}", bool_label(config.plugins.enabled));
    println!("plugin directory: {}", config.plugins.directory.display());
    println!("default provider: {}", config.default_provider);

    let provider = config.resolve_provider(None)?;
    println!("default endpoint: {}", provider.config.endpoint);
    println!("default model: {}", provider.config.model);
    println!(
        "default API key: {}",
        redact_secret(provider.api_key.as_deref())
    );
    Ok(())
}

fn init_config(config_path: Option<PathBuf>, force: bool) -> Result<()> {
    let path = write_default_config(config_path, force)?;
    println!("wrote {}", path.display());
    Ok(())
}

fn providers(config_path: Option<PathBuf>) -> Result<()> {
    let config = load_config(config_path)?;
    for (id, provider) in config.providers {
        let key_status = provider
            .api_key_env
            .as_deref()
            .and_then(|name| std::env::var(name).ok())
            .as_deref()
            .map(|_| "set")
            .unwrap_or("missing");
        println!(
            "{}\n  name: {}\n  model: {}\n  endpoint: {}\n  remote allowed: {}\n  api key: {}\n",
            id,
            provider.display_name,
            provider.model,
            provider.endpoint,
            provider.allow_remote,
            key_status
        );
    }
    Ok(())
}

async fn chat(
    config_path: Option<PathBuf>,
    provider_id: Option<String>,
    prompt: String,
    system: Option<String>,
) -> Result<()> {
    let config = load_config(config_path)?;
    let provider = config.resolve_provider(provider_id.as_deref())?;
    let client = OpenAiCompatibleClient::new();
    let messages = vec![
        ChatMessage::system(system.unwrap_or_else(default_system_prompt)),
        ChatMessage::user(prompt),
    ];

    if config.privacy.log_network_destinations {
        eprintln!("network destination: {}", provider.config.endpoint);
    }

    let response = client
        .chat(ChatCompletionRequest {
            endpoint: provider.config.endpoint,
            model: provider.config.model,
            api_key: provider.api_key,
            allow_remote: provider.config.allow_remote,
            timeout: Duration::from_secs(provider.config.request_timeout_secs),
            messages,
        })
        .await
        .context("chat request failed")?;

    if let Some(model) = response.model {
        eprintln!("model: {model}");
    }
    println!("{}", response.text);
    Ok(())
}

fn validate_plugin(manifest: PathBuf) -> Result<()> {
    let manifest = validate_manifest(&manifest)?;
    println!(
        "plugin '{}' ({}) is valid with {} permission(s)",
        manifest.name,
        manifest.id,
        manifest.permissions.len()
    );
    Ok(())
}

fn blocks_demo() -> Result<()> {
    let mut timeline = BlockTimeline::new();
    timeline.start_command("demo-1", "/tmp/sovereign-term", "cargo test", 1_000)?;
    timeline.append_output_bytes("demo-1", OutputStream::Stdout, b"running 4 tests\n", 1_050)?;
    timeline.append_output_bytes(
        "demo-1",
        OutputStream::Stderr,
        b"test terminal_snapshot_builds_agent_context ... ok\n",
        1_100,
    )?;
    timeline.finish_command("demo-1", 0, 1_250)?;

    println!("{}", serde_json::to_string_pretty(&timeline)?);
    println!("\n--- agent context ---");
    println!("{}", timeline.agent_context_for_blocks(["demo-1"]));
    Ok(())
}

fn git_snapshot_command(path: PathBuf) -> Result<()> {
    let snapshot = git_snapshot(path)?;
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}

fn fs_snapshot_command(
    path: PathBuf,
    max_depth: usize,
    max_entries: usize,
    include_hidden: bool,
) -> Result<()> {
    let policy = FileSnapshotPolicy {
        max_depth,
        max_entries,
        include_hidden,
        ..FileSnapshotPolicy::default()
    };
    let snapshot = snapshot_tree(path, policy)?;
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}

fn default_system_prompt() -> String {
    "You are Sovereign Term, a local-first terminal agent. Be concise, explicit about shell risk, and never imply that data leaves the machine unless a remote provider is configured.".to_string()
}

fn bool_label(value: bool) -> &'static str {
    if value { "enabled" } else { "disabled" }
}

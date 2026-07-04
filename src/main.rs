use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use sovereign_agent::{
    ChatCompletionRequest, ChatMessage, EndpointScope, OpenAiCompatibleClient,
    classify_endpoint_url,
};
use sovereign_core::{SovereignConfig, load_config, redact_secret, write_default_config};
use sovereign_fs::{FileSnapshotPolicy, snapshot_tree};
use sovereign_git::{diff_summary as git_diff_summary, snapshot as git_snapshot};
use sovereign_plugin::validate_manifest;
use sovereign_terminal::{BlockTimeline, OutputStream};
use sovereign_ui::WorkspaceSurface;
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
    Offline {
        #[command(subcommand)]
        command: OfflineCommands,
    },
    Ui {
        #[command(subcommand)]
        command: UiCommands,
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
    DiffSummary {
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

#[derive(Debug, Subcommand)]
enum OfflineCommands {
    Check,
}

#[derive(Debug, Subcommand)]
enum UiCommands {
    Demo,
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
            GitCommands::DiffSummary { path } => git_diff_summary_command(path),
        },
        Commands::Fs { command } => match command {
            FsCommands::Snapshot {
                path,
                max_depth,
                max_entries,
                include_hidden,
            } => fs_snapshot_command(path, max_depth, max_entries, include_hidden),
        },
        Commands::Offline { command } => match command {
            OfflineCommands::Check => offline_check(cli.config),
        },
        Commands::Ui { command } => match command {
            UiCommands::Demo => ui_demo(),
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

fn offline_check(config_path: Option<PathBuf>) -> Result<()> {
    if let Some(path) = config_path.as_ref()
        && !path.exists()
    {
        bail!(
            "offline check config path does not exist: {}",
            path.display()
        );
    }

    let config = load_config(config_path)?;
    let report = build_offline_readiness_report(&config)?;

    println!("Sovereign Term offline readiness");
    println!("telemetry: {}", bool_label(report.telemetry_enabled));
    println!(
        "cloud handoff: {}",
        bool_label(report.cloud_handoff_enabled)
    );
    println!("default provider: {}", report.default_provider);
    println!("providers:");
    for provider in &report.providers {
        println!(
            "  {}\n    endpoint: {}\n    scope: {}\n    remote allowed: {}\n    default: {}",
            provider.id,
            provider.endpoint,
            endpoint_scope_label(provider.scope),
            provider.allow_remote,
            yes_no(provider.is_default)
        );
    }

    if report.problems.is_empty() {
        println!("result: offline-ready");
        return Ok(());
    }

    println!("result: blocked");
    println!("problems:");
    for problem in &report.problems {
        println!("  - {problem}");
    }
    bail!("configuration is not offline-ready")
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct OfflineReadinessReport {
    telemetry_enabled: bool,
    cloud_handoff_enabled: bool,
    default_provider: String,
    providers: Vec<OfflineProviderReport>,
    problems: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OfflineProviderReport {
    id: String,
    endpoint: String,
    scope: EndpointScope,
    allow_remote: bool,
    is_default: bool,
}

fn build_offline_readiness_report(config: &SovereignConfig) -> Result<OfflineReadinessReport> {
    let mut providers = Vec::new();
    let mut problems = Vec::new();

    if config.privacy.telemetry_enabled {
        problems.push("telemetry is enabled".to_string());
    }
    if config.privacy.cloud_handoff_enabled {
        problems.push("cloud handoff is enabled".to_string());
    }

    let mut default_provider_seen = false;
    for (id, provider) in &config.providers {
        let scope = classify_endpoint_url(&provider.endpoint)
            .with_context(|| format!("failed to classify provider '{id}' endpoint"))?;
        let is_default = id == &config.default_provider;
        default_provider_seen |= is_default;

        if is_default && scope == EndpointScope::PublicInternet {
            problems.push(format!(
                "default provider '{id}' points to a public internet endpoint"
            ));
        }
        if scope == EndpointScope::PublicInternet && provider.allow_remote {
            problems.push(format!(
                "provider '{id}' allows public internet endpoint access"
            ));
        }

        providers.push(OfflineProviderReport {
            id: id.clone(),
            endpoint: provider.endpoint.clone(),
            scope,
            allow_remote: provider.allow_remote,
            is_default,
        });
    }

    if !default_provider_seen {
        problems.push(format!(
            "default provider '{}' was not found",
            config.default_provider
        ));
    }

    Ok(OfflineReadinessReport {
        telemetry_enabled: config.privacy.telemetry_enabled,
        cloud_handoff_enabled: config.privacy.cloud_handoff_enabled,
        default_provider: config.default_provider.clone(),
        providers,
        problems,
    })
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

fn git_diff_summary_command(path: PathBuf) -> Result<()> {
    let diff = git_diff_summary(path)?;
    println!("{}", serde_json::to_string_pretty(&diff)?);
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

fn ui_demo() -> Result<()> {
    let surface = WorkspaceSurface::demo_local();
    println!("{}", serde_json::to_string_pretty(&surface)?);
    Ok(())
}

fn default_system_prompt() -> String {
    "You are Sovereign Term, a local-first terminal agent. Be concise, explicit about shell risk, and never imply that data leaves the machine unless a remote provider is configured.".to_string()
}

fn bool_label(value: bool) -> &'static str {
    if value { "enabled" } else { "disabled" }
}

fn endpoint_scope_label(scope: EndpointScope) -> &'static str {
    match scope {
        EndpointScope::Loopback => "loopback",
        EndpointScope::PrivateNetwork => "private-network",
        EndpointScope::PublicInternet => "public-internet",
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sovereign_core::{PluginRuntimeConfig, PrivacyConfig, ProviderConfig};

    use super::*;

    #[test]
    fn default_config_is_offline_ready() {
        let config = SovereignConfig::default_local();
        let report = build_offline_readiness_report(&config).expect("report");

        assert!(report.problems.is_empty());
        assert!(
            report
                .providers
                .iter()
                .all(|provider| provider.scope == EndpointScope::Loopback)
        );
    }

    #[test]
    fn public_remote_provider_opt_in_blocks_offline_readiness() {
        let mut config = SovereignConfig::default_local();
        config.providers.insert(
            "openai".to_string(),
            provider("https://api.openai.com/v1/chat/completions", true),
        );

        let report = build_offline_readiness_report(&config).expect("report");

        assert!(
            report
                .problems
                .iter()
                .any(|problem| problem.contains("provider 'openai' allows public internet"))
        );
    }

    #[test]
    fn public_default_provider_blocks_offline_readiness_even_without_remote_opt_in() {
        let mut providers = BTreeMap::new();
        providers.insert(
            "public-default".to_string(),
            provider("https://api.openai.com/v1/chat/completions", false),
        );
        let config = SovereignConfig {
            default_provider: "public-default".to_string(),
            providers,
            privacy: PrivacyConfig {
                telemetry_enabled: false,
                cloud_handoff_enabled: false,
                log_network_destinations: true,
                require_confirmation_for_shell: true,
            },
            plugins: PluginRuntimeConfig {
                enabled: true,
                directory: PathBuf::from("/tmp/sovereign-term-plugins"),
                allow_unsigned_plugins: false,
            },
        };

        let report = build_offline_readiness_report(&config).expect("report");

        assert!(
            report
                .problems
                .iter()
                .any(|problem| problem.contains("default provider 'public-default'"))
        );
    }

    #[test]
    fn telemetry_and_cloud_handoff_block_offline_readiness() {
        let mut config = SovereignConfig::default_local();
        config.privacy.telemetry_enabled = true;
        config.privacy.cloud_handoff_enabled = true;

        let report = build_offline_readiness_report(&config).expect("report");

        assert!(
            report
                .problems
                .contains(&"telemetry is enabled".to_string())
        );
        assert!(
            report
                .problems
                .contains(&"cloud handoff is enabled".to_string())
        );
    }

    #[test]
    fn offline_check_rejects_explicit_missing_config_path() {
        let missing = std::env::temp_dir().join(format!(
            "sovereign-term-missing-config-for-test-{}.toml",
            std::process::id()
        ));
        let error = offline_check(Some(missing)).expect_err("missing config");

        assert!(error.to_string().contains("config path does not exist"));
    }

    fn provider(endpoint: &str, allow_remote: bool) -> ProviderConfig {
        ProviderConfig {
            display_name: "Test".to_string(),
            endpoint: endpoint.to_string(),
            model: "test-model".to_string(),
            api_key_env: None,
            api_key: None,
            request_timeout_secs: 1,
            allow_remote,
        }
    }
}

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

pub const APP_QUALIFIER: &str = "dev";
pub const APP_ORGANIZATION: &str = "GhotaTechSolutions";
pub const APP_NAME: &str = "SovereignTerm";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SovereignConfig {
    pub default_provider: String,
    pub providers: BTreeMap<String, ProviderConfig>,
    pub privacy: PrivacyConfig,
    pub plugins: PluginRuntimeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub display_name: String,
    pub endpoint: String,
    pub model: String,
    pub api_key_env: Option<String>,
    pub api_key: Option<String>,
    pub request_timeout_secs: u64,
    pub allow_remote: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyConfig {
    pub telemetry_enabled: bool,
    pub cloud_handoff_enabled: bool,
    pub log_network_destinations: bool,
    pub require_confirmation_for_shell: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRuntimeConfig {
    pub enabled: bool,
    pub directory: PathBuf,
    pub allow_unsigned_plugins: bool,
}

#[derive(Debug, Clone)]
pub struct ResolvedProvider {
    pub id: String,
    pub config: ProviderConfig,
    pub api_key: Option<String>,
}

impl SovereignConfig {
    pub fn default_local() -> Self {
        let mut providers = BTreeMap::new();
        providers.insert(
            "omlx".to_string(),
            ProviderConfig {
                display_name: "oMLX Local".to_string(),
                endpoint: "http://127.0.0.1:8000/v1/chat/completions".to_string(),
                model: "ornith-local-agent".to_string(),
                api_key_env: Some("OMLX_API_KEY".to_string()),
                api_key: None,
                request_timeout_secs: 120,
                allow_remote: false,
            },
        );
        providers.insert(
            "omlx-code".to_string(),
            ProviderConfig {
                display_name: "oMLX Code".to_string(),
                endpoint: "http://127.0.0.1:8000/v1/chat/completions".to_string(),
                model: "code-local-agent".to_string(),
                api_key_env: Some("OMLX_API_KEY".to_string()),
                api_key: None,
                request_timeout_secs: 120,
                allow_remote: false,
            },
        );

        Self {
            default_provider: "omlx".to_string(),
            providers,
            privacy: PrivacyConfig {
                telemetry_enabled: false,
                cloud_handoff_enabled: false,
                log_network_destinations: true,
                require_confirmation_for_shell: true,
            },
            plugins: PluginRuntimeConfig {
                enabled: true,
                directory: default_plugins_dir(),
                allow_unsigned_plugins: false,
            },
        }
    }

    pub fn resolve_provider(&self, id: Option<&str>) -> Result<ResolvedProvider> {
        let provider_id = id.unwrap_or(&self.default_provider);
        let config = self
            .providers
            .get(provider_id)
            .with_context(|| format!("provider '{provider_id}' was not found"))?
            .clone();
        let api_key = resolve_api_key(&config)?;

        Ok(ResolvedProvider {
            id: provider_id.to_string(),
            config,
            api_key,
        })
    }
}

pub fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from(APP_QUALIFIER, APP_ORGANIZATION, APP_NAME)
        .context("could not resolve application directories")
}

pub fn default_config_path() -> PathBuf {
    project_dirs()
        .map(|dirs| dirs.config_dir().join("config.toml"))
        .unwrap_or_else(|_| PathBuf::from(".sovereign-term.toml"))
}

pub fn default_plugins_dir() -> PathBuf {
    project_dirs()
        .map(|dirs| dirs.config_dir().join("plugins"))
        .unwrap_or_else(|_| PathBuf::from(".sovereign-term/plugins"))
}

pub fn load_config(path: Option<PathBuf>) -> Result<SovereignConfig> {
    let path = path.unwrap_or_else(default_config_path);
    if !path.exists() {
        return Ok(SovereignConfig::default_local());
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read config at {}", path.display()))?;
    let config = toml::from_str(&raw)
        .with_context(|| format!("failed to parse config at {}", path.display()))?;
    Ok(config)
}

pub fn write_default_config(path: Option<PathBuf>, force: bool) -> Result<PathBuf> {
    let path = path.unwrap_or_else(default_config_path);
    if path.exists() && !force {
        bail!(
            "config already exists at {}. Re-run with --force to overwrite it.",
            path.display()
        );
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    let config = SovereignConfig::default_local();
    let raw = toml::to_string_pretty(&config).context("failed to serialize default config")?;
    fs::write(&path, raw).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn resolve_api_key(config: &ProviderConfig) -> Result<Option<String>> {
    if let Some(env_name) = &config.api_key_env
        && let Ok(value) = env::var(env_name)
        && !value.trim().is_empty()
    {
        return Ok(Some(value));
    }

    if let Some(value) = &config.api_key
        && !value.trim().is_empty()
    {
        return Ok(Some(value.clone()));
    }

    Ok(None)
}

pub fn redact_secret(value: Option<&str>) -> &'static str {
    match value {
        Some(value) if !value.is_empty() => "set",
        _ => "missing",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_local_first() {
        let config = SovereignConfig::default_local();

        assert!(!config.privacy.telemetry_enabled);
        assert!(!config.privacy.cloud_handoff_enabled);
        assert_eq!(config.default_provider, "omlx");

        let provider = config.providers.get("omlx").expect("omlx provider");
        assert_eq!(provider.model, "ornith-local-agent");
        assert_eq!(
            provider.endpoint,
            "http://127.0.0.1:8000/v1/chat/completions"
        );
        assert!(!provider.allow_remote);
    }

    #[test]
    fn resolves_inline_api_key_without_printing_it() {
        let config = ProviderConfig {
            display_name: "Test".to_string(),
            endpoint: "http://127.0.0.1:8000/v1/chat/completions".to_string(),
            model: "test-model".to_string(),
            api_key_env: None,
            api_key: Some("secret".to_string()),
            request_timeout_secs: 10,
            allow_remote: false,
        };

        let key = resolve_api_key(&config).expect("key lookup");
        assert_eq!(key.as_deref(), Some("secret"));
        assert_eq!(redact_secret(key.as_deref()), "set");
    }
}

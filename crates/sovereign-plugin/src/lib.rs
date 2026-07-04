use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub entry: PluginEntry,
    pub permissions: Vec<PluginPermission>,
    pub activation: Vec<ActivationEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    pub kind: PluginEntryKind,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginEntryKind {
    Process,
    Wasm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginPermission {
    pub capability: Capability,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    ReadTerminal,
    WriteTerminal,
    ReadWorkspace,
    WriteWorkspace,
    Shell,
    Network,
    Model,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActivationEvent {
    OnStartup,
    OnCommandBlock,
    OnAgentToolCall,
    Manual,
}

pub fn load_manifest(path: impl AsRef<Path>) -> Result<PluginManifest> {
    let path = path.as_ref();
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read plugin manifest {}", path.display()))?;
    let manifest = toml::from_str(&raw)
        .with_context(|| format!("failed to parse plugin manifest {}", path.display()))?;
    Ok(manifest)
}

pub fn validate_manifest(path: impl AsRef<Path>) -> Result<PluginManifest> {
    let manifest = load_manifest(path)?;
    if manifest.id.trim().is_empty() {
        anyhow::bail!("plugin id cannot be empty");
    }
    if manifest.entry.command.trim().is_empty() {
        anyhow::bail!("plugin entry command cannot be empty");
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_manifest_round_trips_from_toml() {
        let raw = r#"
id = "demo"
name = "Demo"
version = "0.1.0"
activation = ["manual"]

[entry]
kind = "process"
command = "demo-plugin"

[[permissions]]
capability = "read-terminal"
reason = "Reads selected terminal text."
"#;

        let manifest: PluginManifest = toml::from_str(raw).expect("manifest");

        assert_eq!(manifest.id, "demo");
        assert!(matches!(manifest.entry.kind, PluginEntryKind::Process));
        assert!(matches!(
            manifest.permissions[0].capability,
            Capability::ReadTerminal
        ));
    }
}

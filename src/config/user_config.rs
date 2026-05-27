use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub packaging: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub java_version: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_name_pattern: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<String>,
}

pub fn load(path: &Path) -> Result<Option<UserConfig>> {
    match fs::read_to_string(path) {
        Ok(raw) => toml::from_str(&raw)
            .with_context(|| format!("failed to parse spg config at {}", path.display()))
            .map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("failed to read spg config at {}", path.display()))
        }
    }
}

pub fn save(path: &Path, config: &UserConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for spg config at {}",
                parent.display()
            )
        })?;
    }

    let raw = to_toml(config)?;
    fs::write(path, raw)
        .with_context(|| format!("failed to write spg config at {}", path.display()))
}

pub fn remove(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            Err(error).with_context(|| format!("failed to remove spg config at {}", path.display()))
        }
    }
}

pub fn to_toml(config: &UserConfig) -> Result<String> {
    toml::to_string(config).context("failed to serialize spg config as TOML")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn save_and_load_round_trips_toml_config() -> anyhow::Result<()> {
        let path = temp_file("user-config-round-trip");
        let config = UserConfig {
            group_id: Some("com.anas".to_string()),
            language: Some("java".to_string()),
            build: Some("maven".to_string()),
            packaging: Some("jar".to_string()),
            java_version: Some("21".to_string()),
            dependencies: vec!["web".to_string(), "validation".to_string()],
            package_name_pattern: Some("{group_id}.{artifact_id}".to_string()),
            output_dir: Some("~/projects".to_string()),
        };

        save(&path, &config)?;

        assert_eq!(load(&path)?, Some(config));
        let raw = fs::read_to_string(&path)?;
        assert!(raw.contains("group_id = \"com.anas\""));
        assert!(raw.contains("dependencies = [\"web\", \"validation\"]"));

        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
        Ok(())
    }

    #[test]
    fn load_returns_none_when_config_is_missing() -> anyhow::Result<()> {
        assert_eq!(load(&temp_file("missing-user-config"))?, None);
        Ok(())
    }

    fn temp_file(test_name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!("spg-{test_name}-{unique}"))
            .join("config")
            .join("config.toml")
    }
}

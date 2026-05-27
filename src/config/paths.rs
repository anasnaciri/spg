use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub user_config_file: PathBuf,
    pub metadata_cache_file: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let project_dirs = ProjectDirs::from("dev", "spg", "spg")
            .context("could not resolve OS-specific config and cache directories for spg")?;

        Ok(Self::from_dirs(
            project_dirs.config_dir(),
            project_dirs.cache_dir(),
        ))
    }

    pub fn from_dirs(config_dir: impl AsRef<Path>, cache_dir: impl AsRef<Path>) -> Self {
        Self {
            user_config_file: config_dir.as_ref().join("config.toml"),
            metadata_cache_file: cache_dir.as_ref().join("metadata.json"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_separate_config_and_cache_files_from_base_dirs() {
        let paths = AppPaths::from_dirs("/tmp/spg-config", "/tmp/spg-cache");

        assert!(paths.user_config_file.ends_with("config.toml"));
        assert!(paths.metadata_cache_file.ends_with("metadata.json"));
        assert_ne!(
            paths.user_config_file.parent(),
            paths.metadata_cache_file.parent()
        );
    }
}

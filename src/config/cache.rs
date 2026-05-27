use crate::initializr::metadata::InitializrMetadata;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const METADATA_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MetadataCacheFile {
    fetched_at_unix_seconds: u64,
    metadata: InitializrMetadata,
}

pub fn save_metadata_cache(
    path: &Path,
    metadata: &InitializrMetadata,
    fetched_at: SystemTime,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for Spring Initializr metadata cache at {}",
                parent.display()
            )
        })?;
    }

    let cache_file = MetadataCacheFile {
        fetched_at_unix_seconds: unix_seconds(fetched_at)?,
        metadata: metadata.clone(),
    };
    let raw = serde_json::to_vec_pretty(&cache_file)
        .context("failed to serialize Spring Initializr metadata cache")?;

    fs::write(path, raw).with_context(|| {
        format!(
            "failed to write Spring Initializr metadata cache at {}",
            path.display()
        )
    })
}

pub fn load_fresh_metadata_cache(
    path: &Path,
    now: SystemTime,
    ttl: Duration,
) -> Result<Option<InitializrMetadata>> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to read Spring Initializr metadata cache at {}",
                    path.display()
                )
            });
        }
    };

    let cache_file: MetadataCacheFile = serde_json::from_str(&raw).with_context(|| {
        format!(
            "failed to parse Spring Initializr metadata cache at {}",
            path.display()
        )
    })?;
    let now_seconds = unix_seconds(now)?;

    if cache_file.fetched_at_unix_seconds > now_seconds {
        return Ok(Some(cache_file.metadata));
    }

    let age_seconds = now_seconds - cache_file.fetched_at_unix_seconds;
    if age_seconds <= ttl.as_secs() {
        Ok(Some(cache_file.metadata))
    } else {
        Ok(None)
    }
}

pub fn clear_metadata_cache(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| {
            format!(
                "failed to clear Spring Initializr metadata cache at {}",
                path.display()
            )
        }),
    }
}

fn unix_seconds(time: SystemTime) -> Result<u64> {
    Ok(time
        .duration_since(UNIX_EPOCH)
        .context("metadata cache timestamp is before the Unix epoch")?
        .as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::initializr::metadata::{
        Dependency, DependencyGroup, DependencyGroupField, InitializrMetadata,
    };
    use std::{
        fs,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn save_and_load_fresh_metadata_cache_round_trips() -> anyhow::Result<()> {
        let path = temp_file("metadata-cache-round-trip");
        let fetched_at = UNIX_EPOCH + Duration::from_secs(1_000);
        let now = UNIX_EPOCH + Duration::from_secs(1_100);
        let metadata = metadata_with_dependency("web");

        save_metadata_cache(&path, &metadata, fetched_at)?;

        let loaded = load_fresh_metadata_cache(&path, now, Duration::from_secs(24 * 60 * 60))?;

        assert_eq!(loaded, Some(metadata));

        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
        Ok(())
    }

    #[test]
    fn load_fresh_metadata_cache_returns_none_when_expired() -> anyhow::Result<()> {
        let path = temp_file("metadata-cache-expired");
        let fetched_at = UNIX_EPOCH + Duration::from_secs(1_000);
        let now = fetched_at + Duration::from_secs(24 * 60 * 60 + 1);

        save_metadata_cache(&path, &metadata_with_dependency("web"), fetched_at)?;

        let loaded = load_fresh_metadata_cache(&path, now, Duration::from_secs(24 * 60 * 60))?;

        assert_eq!(loaded, None);

        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
        Ok(())
    }

    #[test]
    fn load_fresh_metadata_cache_returns_none_when_missing() -> anyhow::Result<()> {
        let path = temp_file("metadata-cache-missing");

        assert_eq!(
            load_fresh_metadata_cache(
                &path,
                UNIX_EPOCH + Duration::from_secs(1_000),
                Duration::from_secs(24 * 60 * 60),
            )?,
            None
        );

        Ok(())
    }

    #[test]
    fn clear_metadata_cache_deletes_file_when_present() -> anyhow::Result<()> {
        let path = temp_file("clear-cache");
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, "{}")?;

        assert!(clear_metadata_cache(&path)?);
        assert!(!path.exists());

        let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
        Ok(())
    }

    #[test]
    fn clear_metadata_cache_reports_false_when_missing() -> anyhow::Result<()> {
        let path = temp_file("missing-cache");

        assert!(!clear_metadata_cache(&path)?);
        Ok(())
    }

    fn temp_file(test_name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!("spg-{test_name}-{unique}"))
            .join("cache")
            .join("metadata.json")
    }

    fn metadata_with_dependency(id: &str) -> InitializrMetadata {
        InitializrMetadata {
            dependencies: DependencyGroupField {
                values: vec![DependencyGroup {
                    name: "Web".to_string(),
                    values: vec![Dependency {
                        id: id.to_string(),
                        name: "Spring Web".to_string(),
                        description: Some("Build web applications.".to_string()),
                    }],
                }],
            },
            ..InitializrMetadata::default()
        }
    }
}

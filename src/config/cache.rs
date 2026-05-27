use anyhow::{Context, Result};
use std::{fs, io, path::Path};

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

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
}

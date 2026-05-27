use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::{self, Read, Seek},
    path::{Path, PathBuf},
};
use zip::ZipArchive;

pub fn extract_zip_archive<R>(reader: R, destination: &Path) -> Result<()>
where
    R: Read + Seek,
{
    let mut archive = ZipArchive::new(reader).context("failed to read zip archive")?;
    let paths = validated_entry_paths(&mut archive)?;

    fs::create_dir_all(destination).with_context(|| {
        format!(
            "failed to create archive destination at {}",
            destination.display()
        )
    })?;

    for (index, relative_path) in paths.into_iter().enumerate() {
        let mut file = archive
            .by_index(index)
            .with_context(|| format!("failed to read zip entry #{index}"))?;
        let output_path = destination.join(relative_path);

        if file.is_dir() {
            fs::create_dir_all(&output_path).with_context(|| {
                format!(
                    "failed to create directory from zip entry at {}",
                    output_path.display()
                )
            })?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create parent directory for zip entry at {}",
                    parent.display()
                )
            })?;
        }

        let mut output = fs::File::create(&output_path).with_context(|| {
            format!(
                "failed to create file from zip entry at {}",
                output_path.display()
            )
        })?;
        io::copy(&mut file, &mut output)
            .with_context(|| format!("failed to extract zip entry to {}", output_path.display()))?;
    }

    Ok(())
}

fn validated_entry_paths<R>(archive: &mut ZipArchive<R>) -> Result<Vec<PathBuf>>
where
    R: Read + Seek,
{
    let mut paths = Vec::with_capacity(archive.len());

    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .with_context(|| format!("failed to inspect zip entry #{index}"))?;
        let Some(path) = file.enclosed_name() else {
            bail!("unsafe zip entry path: {}", file.name());
        };
        paths.push(path.to_path_buf());
    }

    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::{Cursor, Write},
        time::{SystemTime, UNIX_EPOCH},
    };
    use zip::{ZipWriter, write::SimpleFileOptions};

    #[test]
    fn extracts_files_and_directories_from_zip() -> anyhow::Result<()> {
        let destination = temp_dir("extract-safe");
        let zip = make_zip(&[
            ("demo/", None),
            ("demo/pom.xml", Some("<project />")),
            ("demo/src/main.rs", Some("fn main() {}")),
        ])?;

        extract_zip_archive(Cursor::new(zip), &destination)?;

        assert_eq!(
            fs::read_to_string(destination.join("demo").join("pom.xml"))?,
            "<project />"
        );
        assert_eq!(
            fs::read_to_string(destination.join("demo").join("src").join("main.rs"))?,
            "fn main() {}"
        );

        let _ = fs::remove_dir_all(destination);
        Ok(())
    }

    #[test]
    fn rejects_zip_entries_with_unsafe_paths() -> anyhow::Result<()> {
        let destination = temp_dir("extract-unsafe");
        let zip = make_zip(&[("../evil.txt", Some("bad"))])?;

        let error = extract_zip_archive(Cursor::new(zip), &destination)
            .expect_err("unsafe archive path should fail");

        assert!(error.to_string().contains("unsafe zip entry path"));
        assert!(!destination.join("evil.txt").exists());
        assert!(!destination.parent().unwrap().join("evil.txt").exists());

        let _ = fs::remove_dir_all(destination);
        Ok(())
    }

    fn make_zip(entries: &[(&str, Option<&str>)]) -> anyhow::Result<Vec<u8>> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default();

        for (name, content) in entries {
            match content {
                Some(content) => {
                    writer.start_file(*name, options)?;
                    writer.write_all(content.as_bytes())?;
                }
                None => writer.add_directory(*name, options)?,
            }
        }

        Ok(writer.finish()?.into_inner())
    }

    fn temp_dir(test_name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("spg-{test_name}-{unique}"))
    }
}

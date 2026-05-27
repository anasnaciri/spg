use crate::{
    cli::{CacheCommand, Cli, Commands, ConfigCommand},
    config::{cache, paths::AppPaths, user_config},
    initializr::{client::InitializrClient, metadata::InitializrMetadata},
};
use anyhow::{Context, Result};
use inquire::Confirm;
use std::{
    future::Future,
    io::{self, Write},
    pin::Pin,
};

pub trait Confirmation {
    fn confirm(&mut self, message: &str) -> Result<bool>;
}

pub trait MetadataProvider {
    fn fetch_metadata<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<InitializrMetadata>> + 'a>>;
}

pub struct InquireConfirmation;

impl Confirmation for InquireConfirmation {
    fn confirm(&mut self, message: &str) -> Result<bool> {
        Confirm::new(message)
            .with_default(false)
            .prompt()
            .context("failed to read confirmation prompt")
    }
}

impl MetadataProvider for InitializrClient {
    fn fetch_metadata<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<InitializrMetadata>> + 'a>> {
        Box::pin(async move { InitializrClient::fetch_metadata(self).await })
    }
}

pub async fn run(cli: Cli) -> Result<()> {
    let paths = AppPaths::discover()?;
    let stdout = io::stdout();
    let stderr = io::stderr();
    let mut stdout = stdout.lock();
    let mut stderr = stderr.lock();
    let mut confirmation = InquireConfirmation;

    run_with_paths(cli, &paths, &mut stdout, &mut stderr, &mut confirmation).await
}

pub async fn run_with_paths(
    cli: Cli,
    paths: &AppPaths,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    confirmation: &mut impl Confirmation,
) -> Result<()> {
    let mut metadata_provider = InitializrClient::new_default()?;
    run_with_services(
        cli,
        paths,
        stdout,
        stderr,
        confirmation,
        &mut metadata_provider,
    )
    .await
}

pub async fn run_with_services(
    cli: Cli,
    paths: &AppPaths,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    confirmation: &mut impl Confirmation,
    metadata_provider: &mut impl MetadataProvider,
) -> Result<()> {
    match cli.command {
        Commands::Init(args) => {
            let name = args.project_name.as_deref().unwrap_or("<prompt>");
            writeln!(stderr, "spg init is not implemented yet for {name}")?;
        }
        Commands::Deps => {
            let metadata = metadata_provider.fetch_metadata().await?;
            print_dependencies(&metadata, stdout)?;
        }
        Commands::Config(ConfigCommand::Show) => {
            show_config(paths, stdout)?;
        }
        Commands::Config(ConfigCommand::Reset) => {
            reset_config(paths, stdout, confirmation)?;
        }
        Commands::Cache(CacheCommand::Clear) => {
            clear_cache(paths, stdout)?;
        }
    }

    Ok(())
}

fn print_dependencies(metadata: &InitializrMetadata, stdout: &mut impl Write) -> Result<()> {
    let dependencies = metadata.dependency_entries();

    if dependencies.is_empty() {
        writeln!(stdout, "No Spring Initializr dependencies found")?;
        return Ok(());
    }

    for dependency in dependencies {
        writeln!(
            stdout,
            "{}\t{}\t{}",
            dependency.id, dependency.name, dependency.group
        )?;
    }

    Ok(())
}

fn show_config(paths: &AppPaths, stdout: &mut impl Write) -> Result<()> {
    match user_config::load(&paths.user_config_file)? {
        Some(config) => {
            let raw = user_config::to_toml(&config)?;
            write!(stdout, "{raw}")?;
        }
        None => {
            writeln!(
                stdout,
                "No saved spg config found at {}",
                paths.user_config_file.display()
            )?;
        }
    }

    Ok(())
}

fn reset_config(
    paths: &AppPaths,
    stdout: &mut impl Write,
    confirmation: &mut impl Confirmation,
) -> Result<()> {
    if user_config::load(&paths.user_config_file)?.is_none() {
        writeln!(
            stdout,
            "No saved spg config found at {}",
            paths.user_config_file.display()
        )?;
        return Ok(());
    }

    if confirmation.confirm("Reset saved spg config?")? {
        user_config::remove(&paths.user_config_file)?;
        writeln!(stdout, "Reset saved spg config")?;
    } else {
        writeln!(stdout, "Kept saved spg config")?;
    }

    Ok(())
}

fn clear_cache(paths: &AppPaths, stdout: &mut impl Write) -> Result<()> {
    if cache::clear_metadata_cache(&paths.metadata_cache_file)? {
        writeln!(stdout, "Cleared Spring Initializr metadata cache")?;
    } else {
        writeln!(
            stdout,
            "No Spring Initializr metadata cache found at {}",
            paths.metadata_cache_file.display()
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::Cli,
        config::{paths::AppPaths, user_config::UserConfig},
        initializr::metadata::InitializrMetadata,
    };
    use clap::Parser;
    use std::{
        fs,
        future::Future,
        path::{Path, PathBuf},
        pin::Pin,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[derive(Default)]
    struct StaticConfirmation {
        responses: Vec<bool>,
    }

    impl StaticConfirmation {
        fn yes() -> Self {
            Self {
                responses: vec![true],
            }
        }

        fn no() -> Self {
            Self {
                responses: vec![false],
            }
        }
    }

    impl Confirmation for StaticConfirmation {
        fn confirm(&mut self, _message: &str) -> anyhow::Result<bool> {
            Ok(self.responses.remove(0))
        }
    }

    struct StaticMetadataProvider {
        metadata: InitializrMetadata,
    }

    impl MetadataProvider for StaticMetadataProvider {
        fn fetch_metadata<'a>(
            &'a mut self,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<InitializrMetadata>> + 'a>> {
            let metadata = self.metadata.clone();
            Box::pin(async move { Ok(metadata) })
        }
    }

    #[tokio::test]
    async fn config_show_prints_saved_toml() -> anyhow::Result<()> {
        let paths = temp_paths("config-show");
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
        crate::config::user_config::save(&paths.user_config_file, &config)?;

        let cli = Cli::parse_from(["spg", "config", "show"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();

        run_with_paths(cli, &paths, &mut stdout, &mut stderr, &mut confirmation).await?;

        let output = String::from_utf8(stdout)?;
        assert!(output.contains("group_id = \"com.anas\""));
        assert!(output.contains("dependencies = [\"web\", \"validation\"]"));
        assert!(stderr.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn config_show_reports_when_config_is_missing() -> anyhow::Result<()> {
        let paths = temp_paths("config-show-missing");
        let cli = Cli::parse_from(["spg", "config", "show"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();

        run_with_paths(cli, &paths, &mut stdout, &mut stderr, &mut confirmation).await?;

        let output = String::from_utf8(stdout)?;
        assert!(output.contains("No saved spg config found"));
        assert!(stderr.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn config_reset_deletes_config_after_confirmation() -> anyhow::Result<()> {
        let paths = temp_paths("config-reset");
        crate::config::user_config::save(
            &paths.user_config_file,
            &UserConfig {
                group_id: Some("com.anas".to_string()),
                ..UserConfig::default()
            },
        )?;

        let cli = Cli::parse_from(["spg", "config", "reset"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::yes();

        run_with_paths(cli, &paths, &mut stdout, &mut stderr, &mut confirmation).await?;

        assert!(!paths.user_config_file.exists());
        assert!(String::from_utf8(stdout)?.contains("Reset saved spg config"));
        assert!(stderr.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn config_reset_preserves_config_when_declined() -> anyhow::Result<()> {
        let paths = temp_paths("config-reset-declined");
        crate::config::user_config::save(
            &paths.user_config_file,
            &UserConfig {
                group_id: Some("com.anas".to_string()),
                ..UserConfig::default()
            },
        )?;

        let cli = Cli::parse_from(["spg", "config", "reset"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::no();

        run_with_paths(cli, &paths, &mut stdout, &mut stderr, &mut confirmation).await?;

        assert!(paths.user_config_file.exists());
        assert!(String::from_utf8(stdout)?.contains("Kept saved spg config"));
        assert!(stderr.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn cache_clear_deletes_metadata_cache() -> anyhow::Result<()> {
        let paths = temp_paths("cache-clear");
        fs::create_dir_all(paths.metadata_cache_file.parent().unwrap())?;
        fs::write(&paths.metadata_cache_file, "{}")?;

        let cli = Cli::parse_from(["spg", "cache", "clear"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();

        run_with_paths(cli, &paths, &mut stdout, &mut stderr, &mut confirmation).await?;

        assert!(!paths.metadata_cache_file.exists());
        assert!(String::from_utf8(stdout)?.contains("Cleared Spring Initializr metadata cache"));
        assert!(stderr.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn deps_prints_dependency_catalog_from_metadata_provider() -> anyhow::Result<()> {
        let paths = temp_paths("deps");
        let cli = Cli::parse_from(["spg", "deps"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();
        let mut metadata = StaticMetadataProvider {
            metadata: sample_metadata()?,
        };

        run_with_services(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut metadata,
        )
        .await?;

        let output = String::from_utf8(stdout)?;
        assert!(output.contains("web\tSpring Web\tWeb"));
        assert!(output.contains("data-jpa\tSpring Data JPA\tSQL"));
        assert!(stderr.is_empty());

        cleanup(&paths);
        Ok(())
    }

    fn sample_metadata() -> serde_json::Result<InitializrMetadata> {
        serde_json::from_str(
            r#"
            {
              "dependencies": {
                "values": [
                  {
                    "name": "Web",
                    "values": [
                      {
                        "id": "web",
                        "name": "Spring Web",
                        "description": "Build web applications."
                      }
                    ]
                  },
                  {
                    "name": "SQL",
                    "values": [
                      {
                        "id": "data-jpa",
                        "name": "Spring Data JPA",
                        "description": "Persist data in SQL stores."
                      }
                    ]
                  }
                ]
              }
            }
            "#,
        )
    }

    fn temp_paths(test_name: &str) -> AppPaths {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("spg-{test_name}-{unique}"));
        AppPaths::from_dirs(root.join("config"), root.join("cache"))
    }

    fn cleanup(paths: &AppPaths) {
        remove_ancestor(&paths.user_config_file);
        remove_ancestor(&paths.metadata_cache_file);
    }

    fn remove_ancestor(path: &Path) {
        if let Some(root) = path
            .ancestors()
            .find(|ancestor| ancestor.file_name().is_some_and(|name| name == "config"))
            .and_then(Path::parent)
            .map(PathBuf::from)
        {
            let _ = fs::remove_dir_all(root);
        }
    }
}
